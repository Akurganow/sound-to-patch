//! The protocol between [`super::plugin::PluginRenderer`] and the plug-in
//! host process, `sound-to-patch-host`.
//!
//! The renderer writes one [`Request`] per line to the host's standard
//! input. The host answers each with one [`Reply`] line on standard output,
//! after [`REPLY_PREFIX`]. A plug-in may print to standard output too, so a
//! line without the prefix is plug-in output and is never parsed. Binary
//! state travels as base64; audio travels as a WAV file at a path the
//! renderer names.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::synth::Synth;

/// The start of every reply line.
pub const REPLY_PREFIX: &str = "@sound-to-patch-host ";
/// The version of this protocol. The renderer refuses a host with another
/// version, such as the host of another release.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request to the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Loads the plug-in at `plugin` as `synth`, with the recipe's sample
    /// rate, block size and process mode.
    Open {
        /// The synth the plug-in must be.
        synth: Synth,
        /// The plug-in bundle.
        plugin: PathBuf,
    },
    /// Loads `state`, plays the pre-roll, renders `notes` at `bpm` and
    /// writes the clip to `audio` as a 32-bit float WAV file.
    Render {
        /// The plug-in state, base64. `None` renders the plug-in's own init
        /// patch, before any state load.
        state: Option<String>,
        /// The notes.
        notes: Vec<WireNote>,
        /// Controller changes: pitch bend and mod wheel.
        #[serde(default)]
        controls: Vec<WireControl>,
        /// The tempo in beats per minute.
        bpm: f64,
        /// Where to write the clip.
        audio: PathBuf,
    },
    /// Saves the plug-in state.
    SaveState,
    /// Ends the host.
    Quit,
}

/// A note on the wire. The host checks it as [`super::RenderNote::new`]
/// does.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireNote {
    /// The MIDI key.
    pub key: u8,
    /// The start in seconds.
    pub onset: f64,
    /// The end in seconds.
    pub offset: f64,
    /// The MIDI velocity.
    pub velocity: u8,
}

/// A controller change on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireControlKind {
    /// Pitch bend, 0 to 16383 with the centre at 8192.
    PitchBend {
        /// The bend value.
        value: u16,
    },
    /// The mod wheel, MIDI controller 1, 0 to 127.
    ModWheel {
        /// The wheel value.
        value: u8,
    },
}

/// A controller change at a time of the clip.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireControl {
    /// The time in seconds.
    pub time: f64,
    /// The change.
    #[serde(flatten)]
    pub kind: WireControlKind,
}

/// A reply from the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    /// The plug-in loaded.
    Opened {
        /// The plug-in's name.
        name: String,
        /// The plug-in's build, such as `1.0.7`.
        build: String,
        /// The host and its version.
        host: String,
        /// The architecture of the host process.
        arch: String,
        /// The protocol version the host speaks, [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The recipe version the host renders with.
        recipe_version: u32,
    },
    /// The clip is written.
    Rendered {
        /// The clip length in frames.
        frames: usize,
        /// Whether the tail reached its cap.
        capped: bool,
    },
    /// The plug-in state, base64.
    Saved {
        /// The state.
        state: String,
    },
    /// The host is ending.
    Bye,
    /// The request failed.
    Failed {
        /// Why.
        cause: String,
        /// How to fix it.
        fix: String,
    },
}

/// Writes a reply as one line, prefix included.
///
/// # Errors
///
/// Returns the serializer's error; a reply of plain fields always
/// serializes.
pub fn reply_line(reply: &Reply) -> Result<String, serde_json::Error> {
    Ok(format!("{REPLY_PREFIX}{}", serde_json::to_string(reply)?))
}

/// Reads a reply line. Returns `None` for a line without the prefix, which
/// is plug-in output.
///
/// Plug-in output without a line end may come before the prefix on the
/// same line; it comes back first, apart from the reply.
#[must_use]
pub fn parse_reply(line: &str) -> Option<(&str, Result<Reply, serde_json::Error>)> {
    let at = line.find(REPLY_PREFIX)?;
    let (output, reply) = line.split_at(at);
    Some((output, serde_json::from_str(&reply[REPLY_PREFIX.len()..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_one_tagged_json_line() {
        let open = Request::Open {
            synth: Synth::Vital,
            plugin: PathBuf::from("Vital.vst3"),
        };
        let line = serde_json::to_string(&open).unwrap();
        assert_eq!(
            line,
            r#"{"op":"open","synth":"vital","plugin":"Vital.vst3"}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), open);
    }

    #[test]
    fn plug_in_output_is_not_a_reply() {
        assert!(parse_reply("Vital: loading wavetables").is_none());
        let line = reply_line(&Reply::Rendered {
            frames: 176_400,
            capped: false,
        })
        .unwrap();
        let rendered = Reply::Rendered {
            frames: 176_400,
            capped: false,
        };
        let (output, reply) = parse_reply(&line).unwrap();
        assert_eq!((output, reply.unwrap()), ("", rendered.clone()));
        let joined = format!("Vital: no line end{line}");
        let (output, reply) = parse_reply(&joined).unwrap();
        assert_eq!((output, reply.unwrap()), ("Vital: no line end", rendered));
        assert!(parse_reply("@sound-to-patch-host {").unwrap().1.is_err());
    }
}
