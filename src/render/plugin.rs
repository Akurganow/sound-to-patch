//! The renderer that plays a job through a real plug-in.
//!
//! The plug-in runs in a child process, `sound-to-patch-host`, so a crash
//! or a hang of the plug-in never takes this process down, and the host can
//! run as another architecture. The two talk the [`super::wire`] protocol.
//!
//! The host picks its architecture from the plug-in bundle: a bundle with a
//! slice for this process's architecture loads directly; a bundle with only
//! an `x86_64` slice loads in an `x86_64` host under Rosetta on Apple
//! silicon.
//! Rendering through a plug-in runs on macOS only.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::wire::{self, PROTOCOL_VERSION, Reply, Request, WireControl, WireControlKind, WireNote};
use super::{
    ClipEnd, ControlKind, HostIdentity, RECIPE_VERSION, RenderError, RenderJob, RenderRecord,
    Rendered, Renderer,
};
use crate::audio::decode;
use crate::synth::Synth;
use crate::units::Seconds;

/// The environment variable that names the host binary.
pub const HOST_ENV: &str = "SOUND_TO_PATCH_HOST";
/// The file name of the host binary.
pub const HOST_NAME: &str = "sound-to-patch-host";
/// How many bytes of an executable [`mach_o_archs`] needs: the header of
/// a universal binary with up to 16 slices fits with room to spare.
const HEADER_BYTES: u64 = 4096;
/// How many lines of the host's output an error message quotes.
const TAIL_LINES: usize = 20;
/// How many bytes of one output line an error message quotes.
const TAIL_LINE_BYTES: usize = 1000;
/// The longest line of plug-in output the renderer reads. The rest of a
/// longer line is skipped, so output without line ends cannot grow memory
/// without bound. A reply line is read whole, because a saved state can be
/// many megabytes long.
const MAX_LINE: u64 = 64 * 1024;
/// The Rosetta runtime that runs `x86_64` code on Apple silicon.
const ROSETTA: &str = "/Library/Apple/usr/libexec/oah/libRosettaRuntime";

/// A CPU architecture of a Mach-O binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    /// 64-bit Intel.
    X86_64,
    /// 64-bit ARM, Apple silicon.
    Arm64,
}

impl Arch {
    const CPU_X86_64: u32 = 0x0100_0007;
    const CPU_ARM64: u32 = 0x0100_000c;

    fn of_cpu(cpu: u32) -> Option<Self> {
        match cpu {
            Self::CPU_X86_64 => Some(Self::X86_64),
            Self::CPU_ARM64 => Some(Self::Arm64),
            _ => None,
        }
    }

    /// The architecture of this process.
    #[must_use]
    pub fn current() -> Option<Self> {
        match std::env::consts::ARCH {
            "x86_64" => Some(Self::X86_64),
            "aarch64" => Some(Self::Arm64),
            _ => None,
        }
    }

    /// The name `arch` and `lipo` use.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Arm64 => "arm64",
        }
    }
}

/// Reads the architectures of a Mach-O binary from its header: a thin
/// binary has one, a universal binary lists each slice. Unknown CPU types
/// are left out; a file that is no Mach-O binary gives none.
#[must_use]
pub fn mach_o_archs(bytes: &[u8]) -> Vec<Arch> {
    let be = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_be_bytes)
    };
    let le = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
    };
    match be(0) {
        // A universal binary: 20-byte slice entries, or 32 bytes in the
        // 64-bit form, each starting with the CPU type.
        Some(magic @ (0xcafe_babe | 0xcafe_babf)) => {
            let entry = if magic == 0xcafe_babe { 20 } else { 32 };
            let count = be(4).map_or(0, |n| usize::try_from(n).unwrap_or(0));
            (0..count.min(16))
                .filter_map(|i| be(8 + i * entry).and_then(Arch::of_cpu))
                .collect()
        }
        // A thin 64-bit binary, little-endian.
        Some(0xcffa_edfe) => le(4).and_then(Arch::of_cpu).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The executable inside a plug-in bundle: `Contents/MacOS/<name>`.
///
/// `<name>` is the `CFBundleExecutable` of the bundle's XML `Info.plist`.
/// A binary `Info.plist` gives the bundle's name without `.vst3`. A path
/// that is a file is its own executable.
///
/// # Errors
///
/// Returns [`RenderError::Host`] when `plugin` is a folder without
/// `Contents/Info.plist` or without the executable the bundle names.
pub fn bundle_executable(plugin: &Path) -> Result<PathBuf, RenderError> {
    if plugin.is_file() {
        return Ok(plugin.to_owned());
    }
    let not_a_bundle = |what: String| {
        host_error(
            format!("{} is no plug-in bundle: {what}", plugin.display()),
            "Pass --plugin with the path of the plug-in's .vst3 bundle",
        )
    };
    let info = std::fs::read(plugin.join("Contents").join("Info.plist"))
        .map_err(|_| not_a_bundle("it has no Contents/Info.plist".to_owned()))?;
    let name = plist_executable(&String::from_utf8_lossy(&info))
        .or_else(|| {
            plugin
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let executable = plugin.join("Contents").join("MacOS").join(&name);
    if name.is_empty() || name.contains('/') || !executable.is_file() {
        return Err(not_a_bundle(format!(
            "it has no executable Contents/MacOS/{name}"
        )));
    }
    Ok(executable)
}

/// The `CFBundleExecutable` string of an XML property list.
fn plist_executable(plist: &str) -> Option<String> {
    const KEY: &str = "<key>CFBundleExecutable</key>";
    let after = &plist[plist.find(KEY)? + KEY.len()..];
    let value = after.trim_start().strip_prefix("<string>")?;
    let name = value[..value.find("</string>")?].trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Reads the first [`HEADER_BYTES`] of a file.
fn header(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(HEADER_BYTES)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The standard VST3 bundle of `synth`: the system folder first, then the
/// user's.
#[must_use]
pub fn find_plugin(synth: Synth) -> Option<PathBuf> {
    let bundle = match synth {
        Synth::Vital => "Vital.vst3",
        Synth::Serum2 => "Serum2.vst3",
    };
    let system = PathBuf::from("/Library/Audio/Plug-Ins/VST3");
    let user = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Audio/Plug-Ins/VST3"));
    std::iter::once(system)
        .chain(user)
        .map(|dir| dir.join(bundle))
        .find(|path| path.exists())
}

/// The host binary: the path in [`HOST_ENV`], else the file next to this
/// program.
#[must_use]
pub fn host_binary() -> Option<PathBuf> {
    std::env::var_os(HOST_ENV).map(PathBuf::from).or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(HOST_NAME)))
    })
}

fn host_error(cause: impl Into<String>, fix: impl Into<String>) -> RenderError {
    RenderError::Host {
        cause: cause.into(),
        fix: fix.into(),
    }
}

/// How to start the host for a plug-in: directly, or under `arch` as
/// another architecture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Start the host as it is.
    Direct,
    /// Start the host through `/usr/bin/arch -<arch>`.
    As(Arch),
}

/// Picks how to start the host. `rosetta` says whether this machine can
/// run `x86_64` code on Apple silicon.
///
/// # Errors
///
/// Returns [`RenderError::Host`] when the plug-in has no slice this
/// machine runs, the host binary lacks the slice the plug-in needs, or
/// Rosetta is missing.
pub fn pick_launch(
    plugin: &[Arch],
    host: &[Arch],
    current: Arch,
    rosetta: bool,
) -> Result<Launch, RenderError> {
    if plugin.contains(&current) {
        return Ok(Launch::Direct);
    }
    if current == Arch::Arm64 && plugin.contains(&Arch::X86_64) {
        if !host.contains(&Arch::X86_64) {
            return Err(host_error(
                "the plug-in is x86_64 only, and the host binary has no x86_64 slice",
                "Use the universal sound-to-patch-host from the release archive",
            ));
        }
        if !rosetta {
            return Err(host_error(
                "the plug-in is x86_64 only, and Rosetta is not installed",
                "Install Rosetta with `softwareupdate --install-rosetta`, then render again",
            ));
        }
        return Ok(Launch::As(Arch::X86_64));
    }
    let names: Vec<&str> = plugin.iter().map(|arch| arch.name()).collect();
    Err(host_error(
        format!(
            "the plug-in holds code for [{}], and this machine runs {}",
            names.join(", "),
            current.name()
        ),
        "Install a build of the plug-in for this machine",
    ))
}

/// The last lines a child wrote, kept for error messages.
type Tail = Arc<Mutex<VecDeque<String>>>;

fn keep(tail: &Tail, line: &str) {
    if line.is_empty() {
        return;
    }
    let mut end = line.len().min(TAIL_LINE_BYTES);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    if let Ok(mut lines) = tail.lock() {
        if lines.len() == TAIL_LINES {
            lines.pop_front();
        }
        lines.push_back(line[..end].to_owned());
    }
}

fn quote(tail: &Tail) -> String {
    let lines = tail
        .lock()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join(" | "))
        .unwrap_or_default();
    if lines.is_empty() {
        "none".to_owned()
    } else {
        lines
    }
}

/// How long the renderer waits for the host.
#[derive(Clone, Copy, Debug)]
struct Timeouts {
    /// Loading the plug-in, or answering a short request.
    open: Duration,
    /// A render, on top of the clip's own length.
    render: Duration,
    /// Exiting after its output closed or after `quit`.
    exit: Duration,
}

impl Timeouts {
    const DEFAULT: Self = Self {
        open: Duration::from_secs(60),
        render: Duration::from_secs(120),
        exit: Duration::from_secs(5),
    };
}

/// What arrives on the reply channel: a reply, or a reply line this
/// program cannot read.
type Incoming = Result<Reply, String>;

/// Plays jobs through one plug-in instance in a host process.
#[derive(Debug)]
pub struct PluginRenderer {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Incoming>,
    tail: Tail,
    readers: Vec<JoinHandle<()>>,
    identity: HostIdentity,
    renders: u64,
    loaded: bool,
    timeouts: Timeouts,
    /// A private folder for the clips the host writes.
    scratch: tempfile::TempDir,
}

impl PluginRenderer {
    /// Starts the host at `host` and loads the plug-in at `plugin` as
    /// `synth`.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Host`] when the platform, the plug-in or the
    /// host binary cannot render, and the host's own errors.
    pub fn start(synth: Synth, plugin: &Path, host: &Path) -> Result<Self, RenderError> {
        if !plugin.exists() {
            return Err(host_error(
                format!("there is no plug-in at {}", plugin.display()),
                "Pass --plugin with the path of the plug-in's .vst3 bundle",
            ));
        }
        if !cfg!(target_os = "macos") {
            return Err(host_error(
                "rendering through a plug-in runs on macOS only",
                "Render on a Mac",
            ));
        }
        // `arch` takes no `--`, so an absolute path keeps a host path that
        // starts with `-` from reading as an option.
        let host = std::path::absolute(host).map_err(|source| {
            host_error(
                format!("cannot resolve the host path {} ({source})", host.display()),
                format!("Set {HOST_ENV} to the path of sound-to-patch-host"),
            )
        })?;
        let read = |path: &Path, what: &str| {
            header(path).map_err(|source| {
                host_error(
                    format!("cannot read the {what} at {} ({source})", path.display()),
                    format!("Check the path of the {what}"),
                )
            })
        };
        let executable = bundle_executable(plugin)?;
        let plugin_archs = mach_o_archs(&read(&executable, "plug-in")?);
        if plugin_archs.is_empty() {
            return Err(host_error(
                format!(
                    "{} is no Mach-O binary for x86_64 or arm64",
                    executable.display()
                ),
                "Pass --plugin with the path of the plug-in's .vst3 bundle",
            ));
        }
        let host_archs = mach_o_archs(&read(&host, "host binary")?);
        if host_archs.is_empty() {
            return Err(host_error(
                format!(
                    "the host binary {} is no Mach-O binary for x86_64 or arm64",
                    host.display()
                ),
                format!("Set {HOST_ENV} to the sound-to-patch-host of the release archive"),
            ));
        }
        let current = Arch::current().ok_or_else(|| {
            host_error("this machine's architecture is unknown", "Render on a Mac")
        })?;
        let launch = pick_launch(
            &plugin_archs,
            &host_archs,
            current,
            Path::new(ROSETTA).exists(),
        )?;
        let command = match launch {
            Launch::Direct => Command::new(&host),
            Launch::As(arch) => {
                let mut command = Command::new("/usr/bin/arch");
                command.arg(format!("-{}", arch.name())).arg(&host);
                command
            }
        };
        Self::spawn(command, synth, plugin, Timeouts::DEFAULT)
    }

    /// Runs `command` as the host and loads the plug-in.
    fn spawn(
        mut command: Command,
        synth: Synth,
        plugin: &Path,
        timeouts: Timeouts,
    ) -> Result<Self, RenderError> {
        let mut scratch = tempfile::Builder::new();
        scratch.prefix("sound-to-patch-render-");
        // Only this user may reach the clips, so no one can plant a link
        // where the host writes.
        #[cfg(unix)]
        scratch.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let scratch = scratch.tempdir().map_err(|source| {
            host_error(
                format!("cannot make a folder for the rendered clips ({source})"),
                "Check that the temporary folder is writable",
            )
        })?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| {
                host_error(
                    format!("cannot start {command:?} ({source})"),
                    format!("Check that the host binary runs, or set {HOST_ENV} to its path"),
                )
            })?;
        let (stdin, replies, tail, readers) = connect(&mut child)?;
        let mut renderer = Self {
            child,
            stdin,
            replies,
            tail,
            readers,
            identity: HostIdentity {
                synth,
                build: String::new(),
                host: String::new(),
                arch: String::new(),
            },
            renders: 0,
            loaded: false,
            timeouts,
            scratch,
        };
        let open = Request::Open {
            synth,
            plugin: plugin.to_owned(),
        };
        match renderer.ask(&open, renderer.timeouts.open)? {
            Reply::Opened {
                build,
                host,
                arch,
                protocol,
                recipe_version,
                ..
            } => {
                if protocol != PROTOCOL_VERSION {
                    return Err(another_release(format!(
                        "the host speaks protocol {protocol}, and this program speaks {PROTOCOL_VERSION}"
                    )));
                }
                if recipe_version != RECIPE_VERSION {
                    return Err(another_release(format!(
                        "the host renders with recipe version {recipe_version}, and this program uses {RECIPE_VERSION}"
                    )));
                }
                renderer.identity.build = build;
                renderer.identity.host = host;
                renderer.identity.arch = arch;
                Ok(renderer)
            }
            other => Err(unexpected(&other)),
        }
    }

    /// Sends a request and waits for its reply.
    fn ask(&mut self, request: &Request, timeout: Duration) -> Result<Reply, RenderError> {
        let line = serde_json::to_string(request).map_err(|source| {
            RenderError::Crashed(format!("cannot encode a request ({source})"))
        })?;
        if writeln!(self.stdin, "{line}")
            .and_then(|()| self.stdin.flush())
            .is_err()
        {
            return Err(self.crashed());
        }
        match self.replies.recv_timeout(timeout) {
            Ok(Ok(Reply::Failed { cause, fix })) => Err(RenderError::Host { cause, fix }),
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(line)) => {
                self.stop();
                Err(another_release(format!(
                    "the host sent a reply this program cannot read: {line}"
                )))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.stop();
                Err(RenderError::TimedOut {
                    after: Seconds::from_const(timeout.as_secs_f64()),
                    output: quote(&self.tail),
                })
            }
            Err(RecvTimeoutError::Disconnected) => Err(self.crashed()),
        }
    }

    /// The error of a host whose reply pipe closed. A host that does not
    /// exit soon after is stopped, so this never waits on a live process.
    fn crashed(&mut self) -> RenderError {
        let cause = match self.reap() {
            Some(status) => format!("it exited with {status}"),
            None => "it closed its reply pipe and did not exit, so it was stopped".to_owned(),
        };
        RenderError::Crashed(format!("{cause}; its last output: {}", quote(&self.tail)))
    }

    /// Waits up to the exit timeout for the host to exit, then stops it.
    /// Returns the exit status, or `None` when it had to be stopped.
    fn reap(&mut self) -> Option<String> {
        let deadline = Instant::now() + self.timeouts.exit;
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break Some(status.to_string()),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => break None,
            }
        };
        if status.is_none() {
            self.stop();
        }
        self.join_readers();
        status
    }

    /// Kills the host and waits for it.
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_readers();
    }

    /// Waits up to one second for the pipe readers to finish, so the tail
    /// holds the host's last lines. A reader whose pipe a stray process
    /// still holds open is left behind.
    fn join_readers(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.readers.iter().any(|reader| !reader.is_finished()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let (done, running) = std::mem::take(&mut self.readers)
            .into_iter()
            .partition(JoinHandle::is_finished);
        self.readers = running;
        for reader in done {
            let _ = reader.join();
        }
    }
}

fn unexpected(reply: &Reply) -> RenderError {
    RenderError::Crashed(format!("it gave the reply {reply:?} out of turn"))
}

fn another_release(cause: String) -> RenderError {
    host_error(
        cause,
        "Install the host binary of the same release as this program",
    )
}

/// Reads a pipe line by line until it closes. Bytes that are not UTF-8
/// are replaced, so plug-in output of any kind never stops the reader. A
/// line stops at [`MAX_LINE`] bytes unless `whole` accepts its start.
fn each_line(pipe: impl Read, whole: impl Fn(&[u8]) -> bool, mut line: impl FnMut(&str) -> bool) {
    let mut reader = BufReader::new(pipe);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match read_line(&mut reader, &mut bytes, &whole) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&bytes);
                if !line(text.trim_end_matches(['\n', '\r'])) {
                    break;
                }
            }
        }
    }
}

/// Reads one line of at most [`MAX_LINE`] bytes and skips the rest of a
/// longer one, unless `whole` accepts the first [`MAX_LINE`] bytes.
fn read_line(
    reader: &mut impl BufRead,
    bytes: &mut Vec<u8>,
    whole: impl Fn(&[u8]) -> bool,
) -> io::Result<usize> {
    let read = reader.by_ref().take(MAX_LINE).read_until(b'\n', bytes)?;
    if bytes.last() != Some(&b'\n') && read as u64 == MAX_LINE {
        if whole(bytes) {
            reader.read_until(b'\n', bytes)?;
        } else {
            reader.skip_until(b'\n')?;
        }
    }
    Ok(read)
}

/// Whether a line starts a reply, which may be longer than [`MAX_LINE`].
fn holds_reply(bytes: &[u8]) -> bool {
    let prefix = wire::REPLY_PREFIX.as_bytes();
    bytes.windows(prefix.len()).any(|window| window == prefix)
}

fn read_replies(stdout: impl Read, send: &Sender<Incoming>, tail: &Tail) {
    each_line(stdout, holds_reply, |line| {
        let Some((output, reply)) = wire::parse_reply(line) else {
            keep(tail, line);
            return true;
        };
        keep(tail, output);
        let incoming = reply.map_err(|error| format!("{line} ({error})"));
        send.send(incoming).is_ok()
    });
}

/// The ends of a child's pipes: its input, the reply channel, the tail and
/// the threads that read its output.
type Pipes = (ChildStdin, Receiver<Incoming>, Tail, Vec<JoinHandle<()>>);

/// Wires up the child's pipes: requests go to its input, reply lines come
/// back on a channel, and every other line lands in the tail.
fn connect(child: &mut Child) -> Result<Pipes, RenderError> {
    let missing = || RenderError::Crashed("its pipes did not open".to_owned());
    let stdin = child.stdin.take().ok_or_else(missing)?;
    let stdout = child.stdout.take().ok_or_else(missing)?;
    let stderr = child.stderr.take().ok_or_else(missing)?;
    let tail = Tail::default();
    let (send, replies) = mpsc::channel();
    let out_tail = Arc::clone(&tail);
    let err_tail = Arc::clone(&tail);
    let readers = vec![
        thread::spawn(move || read_replies(stdout, &send, &out_tail)),
        thread::spawn(move || {
            each_line(
                stderr,
                |_| false,
                |line| {
                    keep(&err_tail, line);
                    true
                },
            );
        }),
    ];
    Ok((stdin, replies, tail, readers))
}

impl PluginRenderer {
    /// Renders the job's notes, with its state or with the plug-in's init
    /// patch.
    fn play(&mut self, job: &RenderJob, load: bool) -> Result<Rendered, RenderError> {
        if job.state().synth() != self.identity.synth {
            return Err(RenderError::WrongSynth {
                state: job.state().synth(),
                renderer: self.identity.synth,
            });
        }
        self.renders += 1;
        let audio = self
            .scratch
            .path()
            .join(format!("clip-{}.wav", self.renders));
        let request = Request::Render {
            state: load.then(|| STANDARD.encode(job.state().bytes())),
            notes: job
                .notes()
                .iter()
                .map(|note| WireNote {
                    key: note.key(),
                    onset: note.onset().get(),
                    offset: note.offset().get(),
                    velocity: note.velocity().get(),
                })
                .collect(),
            controls: job
                .controls()
                .iter()
                .map(|control| WireControl {
                    time: control.time.get(),
                    kind: match control.kind {
                        ControlKind::PitchBend(value) => WireControlKind::PitchBend { value },
                        ControlKind::ModWheel(value) => WireControlKind::ModWheel { value },
                    },
                })
                .collect(),
            bpm: job.bpm().get(),
            audio: audio.clone(),
        };
        self.loaded |= load;
        let timeout = self.timeouts.render + Duration::from_secs_f64(job.last_note_off().get());
        let reply = self.ask(&request, timeout);
        let read = reply.and_then(|reply| match reply {
            Reply::Rendered { frames, capped } => {
                let decoded = decode(&audio).map_err(|source| {
                    RenderError::Crashed(format!(
                        "the host wrote a clip that does not decode ({source})"
                    ))
                })?;
                Ok((decoded.into_audio(), ClipEnd { frames, capped }))
            }
            other => Err(unexpected(&other)),
        });
        let _ = std::fs::remove_file(&audio);
        let (audio, end) = read?;
        if audio.frames() != end.frames {
            return Err(RenderError::Crashed(format!(
                "the host reported {} frames and wrote {}",
                end.frames,
                audio.frames()
            )));
        }
        Ok(Rendered {
            audio,
            record: RenderRecord::new(self.identity(), job, end),
        })
    }
}

impl Renderer for PluginRenderer {
    fn identity(&self) -> HostIdentity {
        self.identity.clone()
    }

    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        self.play(job, true)
    }

    fn render_init(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        if self.loaded {
            return Err(host_error(
                "the plug-in already holds a loaded state, so its init patch is gone",
                "Render the init patch before the first render of a preset",
            ));
        }
        self.play(job, false)
    }

    fn save_state(&mut self) -> Result<Vec<u8>, RenderError> {
        match self.ask(&Request::SaveState, self.timeouts.open)? {
            Reply::Saved { state } => STANDARD.decode(state).map_err(|source| {
                RenderError::Crashed(format!(
                    "the host sent a state that is not base64 ({source})"
                ))
            }),
            other => Err(unexpected(&other)),
        }
    }
}

impl Drop for PluginRenderer {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, r#"{{"op":"quit"}}"#);
        let _ = self.stdin.flush();
        let _ = self.replies.recv_timeout(self.timeouts.exit);
        let _ = self.reap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thin(cpu: u32) -> Vec<u8> {
        let mut bytes = 0xfeed_facf_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&cpu.to_le_bytes());
        bytes.extend_from_slice(&[0; 24]);
        bytes
    }

    fn fat(cpus: &[u32]) -> Vec<u8> {
        let mut bytes = 0xcafe_babe_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&u32::try_from(cpus.len()).unwrap().to_be_bytes());
        for cpu in cpus {
            bytes.extend_from_slice(&cpu.to_be_bytes());
            bytes.extend_from_slice(&[0; 16]);
        }
        bytes
    }

    #[test]
    fn the_header_names_each_slice() {
        assert_eq!(mach_o_archs(&thin(Arch::CPU_X86_64)), [Arch::X86_64]);
        assert_eq!(mach_o_archs(&thin(Arch::CPU_ARM64)), [Arch::Arm64]);
        assert_eq!(
            mach_o_archs(&fat(&[Arch::CPU_X86_64, Arch::CPU_ARM64])),
            [Arch::X86_64, Arch::Arm64]
        );
        assert_eq!(mach_o_archs(&fat(&[7, Arch::CPU_ARM64])), [Arch::Arm64]);
        assert_eq!(mach_o_archs(b"#!/bin/sh\n"), []);
        assert_eq!(mach_o_archs(&[0xca, 0xfe]), []);
    }

    #[test]
    fn a_native_slice_loads_directly() {
        let both = [Arch::X86_64, Arch::Arm64];
        assert_eq!(
            pick_launch(&both, &[Arch::Arm64], Arch::Arm64, false).unwrap(),
            Launch::Direct
        );
        assert_eq!(
            pick_launch(&[Arch::X86_64], &[Arch::X86_64], Arch::X86_64, false).unwrap(),
            Launch::Direct
        );
    }

    #[test]
    fn an_x86_64_plug_in_on_apple_silicon_needs_rosetta_and_a_universal_host() {
        let plugin = [Arch::X86_64];
        let universal = [Arch::X86_64, Arch::Arm64];
        assert_eq!(
            pick_launch(&plugin, &universal, Arch::Arm64, true).unwrap(),
            Launch::As(Arch::X86_64)
        );
        let message = |result: Result<Launch, RenderError>| result.unwrap_err().to_string();
        assert!(
            message(pick_launch(&plugin, &[Arch::Arm64], Arch::Arm64, true))
                .contains("universal sound-to-patch-host")
        );
        assert!(
            message(pick_launch(&plugin, &universal, Arch::Arm64, false))
                .contains("softwareupdate --install-rosetta")
        );
        assert!(
            message(pick_launch(&[Arch::Arm64], &universal, Arch::X86_64, true))
                .contains("[arm64]")
        );
    }

    fn lines(input: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        each_line(input, holds_reply, |line| {
            out.push(line.to_owned());
            true
        });
        out
    }

    #[test]
    fn a_long_line_of_output_is_cut_and_a_long_reply_is_read_whole() {
        let long = usize::try_from(MAX_LINE).unwrap() * 3;
        let mut flood = vec![b'x'; long];
        flood.extend_from_slice(b"\nnext\n");
        let read = lines(&flood);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].len(), usize::try_from(MAX_LINE).unwrap());
        assert_eq!(read[1], "next");
        let mut reply = wire::REPLY_PREFIX.as_bytes().to_vec();
        reply.extend(std::iter::repeat_n(b'y', long));
        reply.push(b'\n');
        let read = lines(&reply);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].len(), wire::REPLY_PREFIX.len() + long);
    }

    #[test]
    fn the_tail_keeps_the_start_of_a_long_line() {
        let tail = Tail::default();
        keep(&tail, &"é".repeat(TAIL_LINE_BYTES));
        let quoted = quote(&tail);
        assert!(quoted.len() <= TAIL_LINE_BYTES, "{}", quoted.len());
        assert!(quoted.starts_with('é'));
    }

    fn bundle(plist: &str, executable: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Synth.vst3").join("Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::write(contents.join("Info.plist"), plist).unwrap();
        std::fs::write(contents.join("MacOS").join(executable), b"").unwrap();
        dir
    }

    #[test]
    fn the_bundle_executable_is_the_one_its_plist_names() {
        let plist = "<dict>\n\t<key>CFBundleExecutable</key>\n\t<string>Engine</string>\n</dict>";
        let dir = bundle(plist, "Engine");
        let plugin = dir.path().join("Synth.vst3");
        assert_eq!(
            bundle_executable(&plugin).unwrap(),
            plugin.join("Contents/MacOS/Engine")
        );
        let dir = bundle("bplist00", "Synth");
        let plugin = dir.path().join("Synth.vst3");
        assert_eq!(
            bundle_executable(&plugin).unwrap(),
            plugin.join("Contents/MacOS/Synth")
        );
    }

    #[test]
    fn a_folder_that_is_no_bundle_is_named_as_such() {
        let dir = tempfile::tempdir().unwrap();
        let message = bundle_executable(dir.path()).unwrap_err().to_string();
        assert!(message.contains("is no plug-in bundle: it has no Contents/Info.plist"));
        assert!(message.contains("Pass --plugin"));
        let dir = bundle(
            "<key>CFBundleExecutable</key><string>Gone</string>",
            "Other",
        );
        let message = bundle_executable(&dir.path().join("Synth.vst3"))
            .unwrap_err()
            .to_string();
        assert!(message.contains("it has no executable Contents/MacOS/Gone"));
        let dir = bundle("<key>CFBundleExecutable</key><string>../x</string>", "x");
        assert!(bundle_executable(&dir.path().join("Synth.vst3")).is_err());
    }

    /// A host played by a shell script, for the paths a real plug-in
    /// rarely takes. `$OPENED` holds a valid `opened` reply line.
    #[cfg(unix)]
    mod stub {
        use std::os::unix::fs::PermissionsExt as _;

        use super::*;
        use crate::notes::Velocity;
        use crate::render::{PresetState, RenderNote};
        use crate::units::Bpm;

        const SHORT: Timeouts = Timeouts {
            open: Duration::from_secs(5),
            render: Duration::from_millis(300),
            exit: Duration::from_millis(200),
        };

        fn opened(protocol: u32, recipe: u32) -> String {
            format!(
                r#"{}{{"reply":"opened","name":"Vital","build":"1.0.7","host":"stub","arch":"x86_64","protocol":{protocol},"recipe_version":{recipe}}}"#,
                wire::REPLY_PREFIX
            )
        }

        fn command(script: &str) -> Command {
            let mut command = Command::new("/bin/sh");
            command
                .arg("-c")
                .arg(script)
                .env("OPENED", opened(PROTOCOL_VERSION, RECIPE_VERSION))
                .env("PREFIX", wire::REPLY_PREFIX);
            command
        }

        fn start(script: &str) -> Result<PluginRenderer, RenderError> {
            PluginRenderer::spawn(
                command(script),
                Synth::Vital,
                Path::new("Vital.vst3"),
                SHORT,
            )
        }

        fn job() -> RenderJob {
            let note = RenderNote::new(
                60,
                Seconds::new(0.0).unwrap(),
                Seconds::new(0.1).unwrap(),
                Velocity::new(100).unwrap(),
            )
            .unwrap();
            RenderJob::new(
                PresetState::new(Synth::Vital, b"state".to_vec()),
                vec![note],
                Bpm::new(120.0).unwrap(),
            )
            .unwrap()
        }

        const OPEN: &str = r#"read -r line; printf '%s\n' "$OPENED"; "#;

        #[test]
        fn a_host_that_exits_is_a_crash_with_its_last_output() {
            let error = start("echo 'cannot load' >&2; exit 3").unwrap_err();
            let message = error.to_string();
            assert!(matches!(error, RenderError::Crashed(_)), "{message}");
            assert!(message.contains("exit status: 3"), "{message}");
            assert!(message.contains("cannot load"), "{message}");
        }

        #[test]
        fn a_host_that_hangs_is_killed_and_its_output_quoted() {
            let mut renderer = start(&format!(
                "{OPEN}read -r line; echo 'stuck in a loop' >&2; exec sleep 30"
            ))
            .unwrap();
            let before = Instant::now();
            let error = renderer.render(&job()).unwrap_err();
            assert!(
                before.elapsed() < Duration::from_secs(3),
                "{:?}",
                before.elapsed()
            );
            let RenderError::TimedOut { output, .. } = &error else {
                panic!("{error}");
            };
            assert!(output.contains("stuck in a loop"), "{error}");
            assert!(renderer.child.try_wait().unwrap().is_some());
        }

        #[test]
        fn output_that_is_not_utf8_or_has_no_line_end_keeps_the_reply() {
            let script = r#"read -r line; printf '\377\376 noise'; printf 'no line end'; printf '%s\n' "$OPENED"; read -r line; exit 4"#;
            let mut renderer = start(script).unwrap();
            let message = renderer.render(&job()).unwrap_err().to_string();
            assert!(message.contains("exit status: 4"), "{message}");
            assert!(message.contains("noise"), "{message}");
            assert!(message.contains("no line end"), "{message}");
        }

        #[test]
        fn a_host_of_another_release_is_refused() {
            let other = |reply: String| {
                let mut command =
                    command(r#"read -r line; printf '%s\n' "$REPLY_LINE"; exec sleep 30"#);
                command.env("REPLY_LINE", reply);
                PluginRenderer::spawn(command, Synth::Vital, Path::new("Vital.vst3"), SHORT)
                    .unwrap_err()
                    .to_string()
            };
            let message = other(opened(PROTOCOL_VERSION, RECIPE_VERSION + 1));
            assert!(message.contains("recipe version"), "{message}");
            assert!(message.contains("same release"), "{message}");
            let message = other(opened(PROTOCOL_VERSION + 1, RECIPE_VERSION));
            assert!(message.contains("protocol"), "{message}");
            let message = other(format!(
                r#"{}{{"reply":"opened","name":"Vital"}}"#,
                wire::REPLY_PREFIX
            ));
            assert!(message.contains("cannot read"), "{message}");
            assert!(message.contains("same release"), "{message}");
        }

        #[test]
        fn a_host_that_closes_its_pipes_and_lingers_is_stopped() {
            let mut renderer = start(&format!(
                "{OPEN}read -r line; exec 1>&- 2>&-; exec sleep 30"
            ))
            .unwrap();
            let before = Instant::now();
            let message = renderer.render(&job()).unwrap_err().to_string();
            assert!(
                before.elapsed() < Duration::from_secs(3),
                "{:?}",
                before.elapsed()
            );
            assert!(
                message.contains("did not exit, so it was stopped"),
                "{message}"
            );
        }

        #[test]
        fn a_host_that_never_answers_quit_is_stopped() {
            let renderer = start(&format!("{OPEN}read -r line; exec sleep 30")).unwrap();
            let before = Instant::now();
            drop(renderer);
            assert!(
                before.elapsed() < Duration::from_secs(3),
                "{:?}",
                before.elapsed()
            );
        }

        #[test]
        fn a_host_crash_in_the_load_check_is_a_render_fault() {
            let mut renderer = start(&format!(
                "{OPEN}read -r line; echo 'plug-in crashed' >&2; exit 6"
            ))
            .unwrap();
            let fault = crate::render::check::render_checked(&mut renderer, &job()).unwrap_err();
            assert!(
                matches!(
                    fault,
                    crate::render::check::LoadFault::Render(RenderError::Crashed(_))
                ),
                "{fault}"
            );
            assert!(fault.to_string().contains("plug-in crashed"), "{fault}");
        }

        #[test]
        fn a_host_that_lingers_after_bye_is_stopped() {
            let script = format!(
                r#"{OPEN}read -r line; printf '%s{{"reply":"bye"}}\n' "$PREFIX"; exec sleep 30"#
            );
            let renderer = start(&script).unwrap();
            let pid = renderer.child.id();
            let before = Instant::now();
            drop(renderer);
            assert!(
                before.elapsed() < Duration::from_secs(3),
                "{:?}",
                before.elapsed()
            );
            let alive = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(!alive.success());
        }

        #[test]
        fn a_clip_that_does_not_decode_is_removed() {
            let mark = tempfile::tempdir().unwrap();
            let marked = mark.path().join("audio");
            let script = format!(
                r#"{OPEN}read -r line; path=$(printf '%s' "$line" | sed 's/.*"audio":"\([^"]*\)".*/\1/'); printf junk > "$path"; printf '%s' "$path" > "$MARK"; printf '%s{{"reply":"rendered","frames":1,"capped":false}}\n' "$PREFIX"; read -r line"#
            );
            let mut command = command(&script);
            command.env("MARK", &marked);
            let mut renderer =
                PluginRenderer::spawn(command, Synth::Vital, Path::new("Vital.vst3"), SHORT)
                    .unwrap();
            let message = renderer.render(&job()).unwrap_err().to_string();
            assert!(message.contains("does not decode"), "{message}");
            let clip = PathBuf::from(std::fs::read_to_string(&marked).unwrap());
            assert!(!clip.exists());
            let folder = clip.parent().unwrap().to_owned();
            assert_eq!(folder, renderer.scratch.path());
            let mode = std::fs::metadata(&folder).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "{mode:o}");
            drop(renderer);
            assert!(!folder.exists());
        }
    }
}
