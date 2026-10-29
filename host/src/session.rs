//! One plug-in instance and the renders it plays.
//!
//! The order follows the plug-in's rules: the state loads before
//! processing starts, and the pre-roll and the clip play between one start
//! and one stop, because a stop deactivates the plug-in. The output bus
//! starts inactive after the load, so the host activates it, or the plug-in
//! renders silence.

use std::path::Path;
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sound_to_patch_core::audio::{Audio, write_wav};
use sound_to_patch_core::notes::Velocity;
use sound_to_patch_core::render::wire::{
    PROTOCOL_VERSION, Reply, Request, WireControl, WireControlKind, WireNote,
};
use sound_to_patch_core::render::{
    ControlKind, EventKind, PreRoll, RECIPE_VERSION, Recipe, RenderNote, TailWatch, frame_of,
    note_events,
};
use sound_to_patch_core::synth::Synth;
use sound_to_patch_core::units::Seconds;
use vst3_host::{
    AudioBuffers, BusDirection, MediaType, MidiChannel, MidiEvent, Plugin, ProcessMode,
    TransportPosition, Vst3Host,
};

use crate::{Handler, failed};

/// The version of `vst3-host`, the exact pin in `host/Cargo.toml`.
const VST3_HOST: &str = "0.10.0";

/// The handler: at most one open plug-in.
#[derive(Default)]
pub(crate) struct Sessions {
    open: Option<Session>,
}

impl Handler for Sessions {
    fn handle(&mut self, request: Request) -> Reply {
        let result = match request {
            Request::Open { synth, plugin } => {
                Session::open(synth, &plugin).map(|(session, reply)| {
                    self.open = Some(session);
                    reply
                })
            }
            Request::Render {
                state,
                notes,
                controls,
                bpm,
                audio,
            } => self.session().and_then(|session| {
                session.render(state.as_deref(), &notes, &controls, bpm, &audio)
            }),
            Request::SaveState => self.session().and_then(Session::save_state),
            Request::Quit => Ok(Reply::Bye),
        };
        result.unwrap_or_else(|failure| failure)
    }
}

impl Sessions {
    fn session(&mut self) -> Result<&mut Session, Reply> {
        self.open
            .as_mut()
            .ok_or_else(|| failed("no plug-in is open", "Send an open request first"))
    }
}

/// A failed step, as the reply that says so.
fn step(what: &str) -> impl Fn(vst3_host::Error) -> Reply + '_ {
    move |error| {
        failed(
            format!("{what} failed ({error})"),
            "Check that the plug-in loads in a DAW, then render again",
        )
    }
}

/// The plug-in name of each synth.
fn plugin_name(synth: Synth) -> &'static str {
    match synth {
        Synth::Vital => "Vital",
        Synth::Serum2 => "Serum 2",
    }
}

struct Session {
    // Declared before the host, so the plug-in drops first.
    plugin: Plugin,
    _host: Vst3Host,
    recipe: Recipe,
    buffers: AudioBuffers,
}

impl Session {
    fn open(synth: Synth, path: &Path) -> Result<(Self, Reply), Reply> {
        let recipe = Recipe::current();
        let rate = f64::from(recipe.sample_rate().get());
        let block = usize::try_from(recipe.block_size()).unwrap_or(512);
        let mut host = Vst3Host::builder()
            .sample_rate(rate)
            .block_size(block)
            .build()
            .map_err(step("starting the VST3 host"))?;
        let mut plugin = host
            .load_plugin(path)
            .map_err(step("loading the plug-in"))?;
        let info = plugin.info().clone();
        if info.name != plugin_name(synth) {
            return Err(failed(
                format!(
                    "the plug-in at {} is {}, not {}",
                    path.display(),
                    info.name,
                    plugin_name(synth)
                ),
                format!(
                    "Pass --plugin with the path of the {} plug-in",
                    plugin_name(synth)
                ),
            ));
        }
        plugin
            .set_bus_active(MediaType::Audio, BusDirection::Output, 0, true)
            .map_err(step("activating the output bus"))?;
        if plugin.output_channel_count() < 2 {
            return Err(failed(
                format!(
                    "the plug-in has {} output channels",
                    plugin.output_channel_count()
                ),
                "Render with a stereo instrument plug-in",
            ));
        }
        plugin
            .set_process_mode(ProcessMode::Offline)
            .map_err(step("setting offline processing"))?;
        let buffers = AudioBuffers::new(0, plugin.output_channel_count(), block, rate);
        let reply = Reply::Opened {
            name: info.name,
            build: info.version,
            host: format!(
                "sound-to-patch-host {} (vst3-host {VST3_HOST})",
                env!("CARGO_PKG_VERSION")
            ),
            arch: std::env::consts::ARCH.to_owned(),
            protocol: PROTOCOL_VERSION,
            recipe_version: RECIPE_VERSION,
        };
        Ok((
            Self {
                plugin,
                _host: host,
                recipe,
                buffers,
            },
            reply,
        ))
    }

    fn save_state(&mut self) -> Result<Reply, Reply> {
        let state = self.plugin.save_state().map_err(step("saving the state"))?;
        Ok(Reply::Saved {
            state: STANDARD.encode(state),
        })
    }

    fn render(
        &mut self,
        state: Option<&str>,
        notes: &[WireNote],
        controls: &[WireControl],
        bpm: f64,
        audio: &Path,
    ) -> Result<Reply, Reply> {
        let bad = |cause: String| failed(cause, "Report it; the renderer checks every request");
        let state = state
            .map(|state| {
                STANDARD
                    .decode(state)
                    .map_err(|error| bad(format!("the state is not base64 ({error})")))
            })
            .transpose()?;
        let notes = notes
            .iter()
            .map(|note| {
                let seconds = |t: f64| Seconds::new(t).map_err(|error| bad(error.to_string()));
                let velocity =
                    Velocity::new(note.velocity).map_err(|error| bad(error.to_string()))?;
                RenderNote::new(
                    note.key,
                    seconds(note.onset)?,
                    seconds(note.offset)?,
                    velocity,
                )
                .map_err(|error| bad(error.to_string()))
            })
            .collect::<Result<Vec<_>, Reply>>()?;
        if notes.is_empty() {
            return Err(bad("the request holds no notes".to_owned()));
        }
        let state_load_s = state
            .map(|state| {
                let start = Instant::now();
                self.plugin
                    .load_state(&state)
                    .map_err(step("loading the state"))?;
                Ok(start.elapsed().as_secs_f64())
            })
            .transpose()?;
        let rate = self.recipe.sample_rate();
        let controls = controls
            .iter()
            .map(|control| {
                let time = Seconds::new(control.time).map_err(|error| bad(error.to_string()))?;
                ControlKind::from(control.kind)
                    .checked()
                    .map_err(|error| bad(error.to_string()))?;
                Ok((frame_of(time, rate), control_event(control.kind)))
            })
            .collect::<Result<Vec<_>, Reply>>()?;
        self.plugin
            .set_tempo(bpm)
            .map_err(step("setting the tempo"))?;
        self.plugin
            .start_processing()
            .map_err(step("starting processing"))?;
        let clip = self.pre_roll().and_then(|()| self.clip(&notes, controls));
        let stop = self
            .plugin
            .stop_processing()
            .map_err(step("stopping processing"));
        let (channels, frames, capped) = clip?;
        stop?;
        let audio_data = Audio::new(self.recipe.sample_rate(), channels).map_err(|error| {
            bad(format!(
                "the plug-in gave audio that is not valid ({error})"
            ))
        })?;
        write_wav(audio, &audio_data).map_err(|error| {
            failed(
                format!("cannot write {} ({error})", audio.display()),
                "Check that the folder exists and is writable",
            )
        })?;
        Ok(Reply::Rendered {
            frames,
            capped,
            state_load_s,
        })
    }

    /// Plays the recipe's pre-roll and throws its audio away. Each step
    /// runs whole blocks, so it lasts up to one block longer than the
    /// recipe says; the clip itself starts on a block boundary.
    fn pre_roll(&mut self) -> Result<(), Reply> {
        let rate = self.recipe.sample_rate();
        // A controller keeps its value across renders, so the pre-roll
        // centres the bend and zeroes the mod wheel first.
        let reset = [
            (
                0,
                control_event(WireControlKind::PitchBend { value: CENTRE }),
            ),
            (0, control_event(WireControlKind::ModWheel { value: 0 })),
        ];
        for (index, part) in self.recipe.pre_roll().to_vec().into_iter().enumerate() {
            let (length, events) = match part {
                PreRoll::Settle { length } => (length, Vec::new()),
                PreRoll::WarmUp {
                    key,
                    velocity,
                    start,
                    end,
                    length,
                } => (
                    length,
                    vec![
                        (frame_of(start, rate), note_on(key, velocity)),
                        (frame_of(end, rate), note_off(key)),
                    ],
                ),
            };
            let events = if index == 0 {
                reset.iter().copied().chain(events).collect()
            } else {
                events
            };
            let frames = frame_of(length, rate);
            self.play(&events, |frame, _| Ok(frame + 1 >= frames))?;
        }
        Ok(())
    }

    /// Plays the notes from the start of the transport until the tail
    /// watch ends the clip. Returns the two channels, the length and
    /// whether the tail was capped.
    fn clip(
        &mut self,
        notes: &[RenderNote],
        controls: Vec<(usize, MidiEvent)>,
    ) -> Result<(Vec<Vec<f64>>, usize, bool), Reply> {
        let rate = self.recipe.sample_rate();
        self.plugin
            .set_transport_position(TransportPosition {
                samples: 0,
                quarter_notes: 0.0,
            })
            .map_err(step("moving the transport to the start"))?;
        // Controllers come before the notes of their frame, so a bend at a
        // note's start applies to the note. The sort keeps that order.
        let mut events = controls;
        events.extend(note_events(notes, rate).iter().map(|event| {
            let midi = match event.kind {
                EventKind::On => note_on(event.key, event.velocity.get()),
                EventKind::Off => note_off(event.key),
            };
            (event.frame, midi)
        }));
        events.sort_by_key(|(frame, _)| *frame);
        let mut watch: TailWatch = self.recipe.tail_watch(notes);
        let mut channels = vec![Vec::new(), Vec::new()];
        let mut end = None;
        self.play(&events, |_, samples| {
            channels[0].push(samples[0]);
            channels[1].push(samples[1]);
            end = watch.push(&samples).map_err(|error| {
                failed(
                    format!("the plug-in gave audio that is not valid ({error})"),
                    "Report it with the preset",
                )
            })?;
            Ok(end.is_some())
        })?;
        let end = end.ok_or_else(|| failed("the clip did not end", "Report it"))?;
        Ok((channels, end.frames, end.capped))
    }

    /// Processes blocks from frame 0, sending each event in its block, and
    /// hands each output frame to `sink` until it returns true.
    fn play(
        &mut self,
        events: &[(usize, MidiEvent)],
        mut sink: impl FnMut(usize, [f64; 2]) -> Result<bool, Reply>,
    ) -> Result<(), Reply> {
        let block = self.buffers.block_size;
        let mut start = 0;
        loop {
            for (frame, event) in events
                .iter()
                .filter(|(frame, _)| (start..start + block).contains(frame))
            {
                let offset = i32::try_from(frame - start).unwrap_or(0);
                self.plugin
                    .send_midi_event_at(*event, offset)
                    .map_err(step("sending a note"))?;
            }
            for channel in &mut self.buffers.outputs {
                channel.fill(0.0);
            }
            self.plugin
                .process_audio(&mut self.buffers)
                .map_err(step("processing a block"))?;
            for i in 0..block {
                let samples = [
                    f64::from(self.buffers.outputs[0][i]),
                    f64::from(self.buffers.outputs[1][i]),
                ];
                if sink(start + i, samples)? {
                    return Ok(());
                }
            }
            start += block;
        }
    }
}

fn note_on(key: u8, velocity: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: MidiChannel::Ch1,
        note: key,
        velocity,
    }
}

/// The centre of the pitch bend range.
const CENTRE: u16 = 8192;

fn control_event(kind: WireControlKind) -> MidiEvent {
    match kind {
        WireControlKind::PitchBend { value } => MidiEvent::PitchBend {
            channel: MidiChannel::Ch1,
            value,
        },
        WireControlKind::ModWheel { value } => MidiEvent::ControlChange {
            channel: MidiChannel::Ch1,
            controller: 1,
            value,
        },
    }
}

fn note_off(key: u8) -> MidiEvent {
    MidiEvent::NoteOff {
        channel: MidiChannel::Ch1,
        note: key,
        velocity: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vst3_host_version_is_the_pin() {
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains(&format!("vst3-host = {{ version = \"={VST3_HOST}\"")));
    }
}
