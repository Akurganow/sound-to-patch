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
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::wire::{self, Reply, Request, WireNote};
use super::{
    ClipEnd, HostIdentity, RECIPE_VERSION, RenderError, RenderJob, RenderRecord, Rendered, Renderer,
};
use crate::audio::decode;
use crate::synth::Synth;
use crate::units::Seconds;

/// The environment variable that names the host binary.
pub const HOST_ENV: &str = "SOUND_TO_PATCH_HOST";
/// The file name of the host binary.
pub const HOST_NAME: &str = "sound-to-patch-host";
/// How long the host may take to load the plug-in or answer a short
/// request.
const OPEN_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a render may take on top of the clip's own length.
const RENDER_TIMEOUT: Duration = Duration::from_secs(120);
/// How many lines of the host's output an error message quotes.
const TAIL_LINES: usize = 20;
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

/// The executable inside a plug-in bundle: `Contents/MacOS/<name>`, where
/// `<name>` is the bundle's name without `.vst3`. A path that is a file is
/// its own executable.
#[must_use]
pub fn bundle_executable(plugin: &Path) -> PathBuf {
    if plugin.is_file() {
        return plugin.to_owned();
    }
    let name = plugin.file_stem().unwrap_or_default();
    plugin.join("Contents").join("MacOS").join(name)
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

fn keep(tail: &Tail, line: String) {
    if let Ok(mut lines) = tail.lock() {
        if lines.len() == TAIL_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }
}

fn quote(tail: &Tail) -> String {
    tail.lock()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join(" | "))
        .unwrap_or_default()
}

/// Plays jobs through one plug-in instance in a host process.
#[derive(Debug)]
pub struct PluginRenderer {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Reply>,
    tail: Tail,
    identity: HostIdentity,
    renders: u64,
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
        if !cfg!(target_os = "macos") {
            return Err(host_error(
                "rendering through a plug-in runs on macOS only",
                "Render on a Mac",
            ));
        }
        if !plugin.exists() {
            return Err(host_error(
                format!("there is no plug-in at {}", plugin.display()),
                "Pass --plugin with the path of the plug-in's .vst3 bundle",
            ));
        }
        let read = |path: &Path, what: &str| {
            std::fs::read(path).map_err(|source| {
                host_error(
                    format!("cannot read the {what} at {} ({source})", path.display()),
                    format!("Check the path of the {what}"),
                )
            })
        };
        let plugin_archs = mach_o_archs(&read(&bundle_executable(plugin), "plug-in")?);
        let host_archs = mach_o_archs(&read(host, "host binary")?);
        let current = Arch::current().ok_or_else(|| {
            host_error("this machine's architecture is unknown", "Render on a Mac")
        })?;
        let launch = pick_launch(
            &plugin_archs,
            &host_archs,
            current,
            Path::new(ROSETTA).exists(),
        )?;
        let mut command = match launch {
            Launch::Direct => Command::new(host),
            Launch::As(arch) => {
                let mut command = Command::new("/usr/bin/arch");
                command.arg(format!("-{}", arch.name())).arg(host);
                command
            }
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| {
                host_error(
                    format!("cannot start {} ({source})", host.display()),
                    format!("Check that the host binary runs, or set {HOST_ENV} to its path"),
                )
            })?;
        let (stdin, replies, tail) = connect(&mut child)?;
        let mut renderer = Self {
            child,
            stdin,
            replies,
            tail,
            identity: HostIdentity {
                synth,
                build: String::new(),
                host: String::new(),
                arch: String::new(),
            },
            renders: 0,
        };
        let open = Request::Open {
            synth,
            plugin: plugin.to_owned(),
        };
        match renderer.ask(&open, OPEN_TIMEOUT)? {
            Reply::Opened {
                build,
                host,
                arch,
                recipe_version,
                ..
            } => {
                if recipe_version != RECIPE_VERSION {
                    return Err(host_error(
                        format!(
                            "the host renders with recipe version {recipe_version}, and this program uses {RECIPE_VERSION}"
                        ),
                        "Install the host binary of the same release as this program",
                    ));
                }
                renderer.identity.build = build;
                renderer.identity.host = host;
                renderer.identity.arch = arch;
                Ok(renderer)
            }
            other => Err(unexpected(&other)),
        }
    }

    /// Returns the state the plug-in saves now.
    ///
    /// # Errors
    ///
    /// Returns the host's error, or [`RenderError::Crashed`] or
    /// [`RenderError::TimedOut`].
    pub fn save_state(&mut self) -> Result<Vec<u8>, RenderError> {
        match self.ask(&Request::SaveState, OPEN_TIMEOUT)? {
            Reply::Saved { state } => STANDARD.decode(state).map_err(|source| {
                RenderError::Crashed(format!(
                    "the host sent a state that is not base64 ({source})"
                ))
            }),
            other => Err(unexpected(&other)),
        }
    }

    /// Sends a request and waits for its reply.
    fn ask(&mut self, request: &Request, timeout: Duration) -> Result<Reply, RenderError> {
        let line = serde_json::to_string(request).map_err(|source| {
            RenderError::Crashed(format!("cannot encode a request ({source})"))
        })?;
        writeln!(self.stdin, "{line}")
            .and_then(|()| self.stdin.flush())
            .map_err(|_| self.crashed())?;
        match self.replies.recv_timeout(timeout) {
            Ok(Reply::Failed { cause, fix }) => Err(RenderError::Host { cause, fix }),
            Ok(reply) => Ok(reply),
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                Err(RenderError::TimedOut(Seconds::from_const(
                    timeout.as_secs_f64(),
                )))
            }
            Err(RecvTimeoutError::Disconnected) => Err(self.crashed()),
        }
    }

    fn crashed(&mut self) -> RenderError {
        let status = self.child.wait().map_or_else(
            |_| "an unknown status".to_owned(),
            |status| status.to_string(),
        );
        RenderError::Crashed(format!(
            "it exited with {status}; its last output: {}",
            quote(&self.tail)
        ))
    }
}

fn unexpected(reply: &Reply) -> RenderError {
    RenderError::Crashed(format!("it gave the reply {reply:?} out of turn"))
}

/// Wires up the child's pipes: requests go to its input, reply lines come
/// back on a channel, and every other line lands in the tail.
fn connect(child: &mut Child) -> Result<(ChildStdin, Receiver<Reply>, Tail), RenderError> {
    let missing = || RenderError::Crashed("its pipes did not open".to_owned());
    let stdin = child.stdin.take().ok_or_else(missing)?;
    let stdout = child.stdout.take().ok_or_else(missing)?;
    let stderr = child.stderr.take().ok_or_else(missing)?;
    let tail = Tail::default();
    let (send, replies) = mpsc::channel();
    let out_tail = Arc::clone(&tail);
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(io::Result::ok) {
            match wire::parse_reply(&line) {
                Some(Ok(reply)) => {
                    if send.send(reply).is_err() {
                        break;
                    }
                }
                Some(Err(error)) => keep(&out_tail, format!("unreadable reply ({error}): {line}")),
                None => keep(&out_tail, line),
            }
        }
    });
    let err_tail = Arc::clone(&tail);
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(io::Result::ok) {
            keep(&err_tail, line);
        }
    });
    Ok((stdin, replies, tail))
}

impl Renderer for PluginRenderer {
    fn identity(&self) -> HostIdentity {
        self.identity.clone()
    }

    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        if job.state().synth() != self.identity.synth {
            return Err(RenderError::WrongSynth {
                state: job.state().synth(),
                renderer: self.identity.synth,
            });
        }
        self.renders += 1;
        let audio = std::env::temp_dir().join(format!(
            "sound-to-patch-render-{}-{}.wav",
            std::process::id(),
            self.renders
        ));
        let request = Request::Render {
            state: STANDARD.encode(job.state().bytes()),
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
            bpm: job.bpm().get(),
            audio: audio.clone(),
        };
        let timeout = RENDER_TIMEOUT + Duration::from_secs_f64(job.last_note_off().get());
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

impl Drop for PluginRenderer {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, r#"{{"op":"quit"}}"#);
        let _ = self.stdin.flush();
        if self.replies.recv_timeout(Duration::from_secs(5)).is_err() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
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

    #[test]
    fn the_bundle_executable_is_named_after_the_bundle() {
        assert_eq!(
            bundle_executable(Path::new("/nowhere/Vital.vst3")),
            Path::new("/nowhere/Vital.vst3/Contents/MacOS/Vital")
        );
    }
}
