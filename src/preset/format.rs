//! The preset file of a synth behind two traits: [`PresetFormat`] reads and
//! writes the bytes, and [`PresetModel`] is the decoded preset.
//!
//! The commands and the edit applier use only these traits. Container,
//! number and hash rules stay inside each adapter. A model keeps every key
//! it read, including keys the registry does not know, so a rewrite loses
//! no feature of a newer build.

use std::any::Any;
use std::error::Error;
use std::fmt::{self, Write as _};

use crate::preset::template::TemplateSource;
use crate::synth::{Synth, SynthError};

/// The file format of one synth.
pub trait PresetFormat: Send + Sync {
    /// The synth the format serves.
    fn synth(&self) -> Synth;

    /// The file extension without the dot.
    fn extension(&self) -> &'static str;

    /// The largest input [`PresetFormat::read`] accepts, in bytes.
    fn max_bytes(&self) -> usize;

    /// Decodes a preset.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] naming the key or offset that fails.
    fn read(&self, bytes: &[u8]) -> Result<Box<dyn PresetModel>, FormatError>;

    /// Decodes only the header fields, without the large parts.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] as [`PresetFormat::read`] gives it.
    fn read_header(&self, bytes: &[u8]) -> Result<Header, FormatError>;

    /// Encodes a model of this synth.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] when the model belongs to another synth, holds a
    /// value the format cannot store or lacks a part the synth needs to
    /// load the file.
    fn write(&self, model: &dyn PresetModel) -> Result<Vec<u8>, FormatError>;

    /// Whether a missing key and an explicit default sound the same.
    fn absent_means_default(&self, key: &str) -> bool;

    /// Where the template of this synth comes from.
    fn template(&self) -> &dyn TemplateSource;
}

/// A decoded preset.
pub trait PresetModel: fmt::Debug + Send + Sync {
    /// The header fields.
    fn header(&self) -> Header;

    /// The value of a control as the file holds it, or `None` when the key
    /// is absent. An explicit default is `Some`.
    fn get(&self, key: &str) -> Option<Stored>;

    /// Sets a control.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] when the format cannot hold the value there. The
    /// model is unchanged then.
    fn set(&mut self, key: &str, value: Stored) -> Result<(), FormatError>;

    /// Removes a control, or writes its default where the format needs one.
    fn reset(&mut self, key: &str);

    /// The modulation slots in order. An empty slot is `None`.
    fn routes(&self) -> Vec<Option<Route>>;

    /// Sets or clears a modulation slot.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] when the slot does not exist or the route does not
    /// fit the format. The model is unchanged then.
    fn set_route(&mut self, slot: usize, route: Option<Route>) -> Result<(), FormatError>;

    /// Sets the preset name.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] when the format cannot hold the text.
    fn set_name(&mut self, name: &str) -> Result<(), FormatError>;

    /// Sets the author.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] when the format cannot hold the text.
    fn set_author(&mut self, author: &str) -> Result<(), FormatError>;

    /// Sets the description.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] when the format cannot hold the text.
    fn set_description(&mut self, description: &str) -> Result<(), FormatError>;

    /// Sets the tags or the style.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] naming the tags the format accepts.
    fn set_tags(&mut self, tags: &[String]) -> Result<(), FormatError>;

    /// The ordered tree that `preset inspect` prints.
    fn describe(&self, opts: &DescribeOptions) -> Description;

    /// Keys in the file that the registry does not cover, as paths such as
    /// `settings.custom_warps`. They stay in the model and survive a write.
    fn unknown_keys(&self) -> Vec<String>;

    /// The model as [`Any`], so its format can take it back for writing.
    fn as_any(&self) -> &dyn Any;
}

/// A value as the file stores it.
#[derive(Clone, Debug, PartialEq)]
pub enum Stored {
    /// A real number.
    Float(f64),
    /// An integer.
    Int(i64),
    /// A flag.
    Flag(bool),
    /// A text.
    Text(String),
}

impl fmt::Display for Stored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float(value) => write!(f, "{value:?}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Flag(value) => write!(f, "{value}"),
            Self::Text(value) => write!(f, "{value:?}"),
        }
    }
}

/// The direction of a modulation source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePolarity {
    /// The source moves the destination one way from its value.
    Unipolar,
    /// The source moves the destination both ways around its value.
    Bipolar,
}

/// One modulation slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// The source name as the file stores it.
    pub source: String,
    /// The destination key as the file stores it.
    pub destination: String,
    /// The amount as the file stores it.
    pub amount: f64,
    /// The polarity of the source.
    pub polarity: RoutePolarity,
    /// Further fields of the slot that one synth has, by key.
    pub extras: Vec<(String, Stored)>,
}

/// How a header field reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// As expected.
    Ok,
    /// Worth a look.
    Warn,
    /// The synth may refuse the file.
    Bad,
}

/// One line of the header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderField {
    /// What the field is.
    pub label: String,
    /// Its value as text.
    pub value: String,
    /// How it reads, when it can be judged.
    pub status: Option<Status>,
}

/// The header fields of a preset, in print order. The first field is
/// always `build`: the build stamp the file carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// The fields.
    pub fields: Vec<HeaderField>,
}

impl Header {
    /// Adds a field.
    pub fn push(&mut self, label: &str, value: impl Into<String>, status: Option<Status>) {
        self.fields.push(HeaderField {
            label: label.to_owned(),
            value: value.into(),
            status,
        });
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self
            .fields
            .iter()
            .map(|field| field.label.len())
            .max()
            .unwrap_or(0);
        for field in &self.fields {
            write!(f, "{:width$}  {}", field.label, field.value)?;
            match field.status {
                Some(Status::Warn) => write!(f, "  [warn]")?,
                Some(Status::Bad) => write!(f, "  [bad]")?,
                Some(Status::Ok) | None => {}
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// How much of a long list or text [`PresetModel::describe`] shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescribeOptions {
    /// Show every element and every character.
    pub full: bool,
    /// Elements of a list shown when `full` is false.
    pub shown: usize,
}

impl Default for DescribeOptions {
    fn default() -> Self {
        Self {
            full: false,
            shown: 3,
        }
    }
}

/// The longest text shown in full when `full` is false.
pub const SHOWN_TEXT_CHARS: usize = 64;

/// A node of the describe tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// Named children in file order.
    Map(Vec<(String, Node)>),
    /// A list with its full length and the elements shown.
    List {
        /// Elements in the file.
        len: usize,
        /// The first elements, or all of them.
        items: Vec<Node>,
    },
    /// A value as text, with an optional mark such as `out of range`.
    Scalar {
        /// The value as text.
        text: String,
        /// A warning about the value.
        mark: Option<String>,
    },
}

impl Node {
    /// A scalar without a mark.
    #[must_use]
    pub fn scalar(text: impl Into<String>) -> Self {
        Self::Scalar {
            text: text.into(),
            mark: None,
        }
    }
}

/// The tree that `preset inspect` prints under the header.
#[derive(Clone, Debug, PartialEq)]
pub struct Description {
    /// The top-level node, a map for every current format.
    pub root: Node,
}

impl fmt::Display for Description {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        render(&self.root, 0, &mut out)?;
        f.write_str(&out)
    }
}

fn render(node: &Node, depth: usize, out: &mut String) -> fmt::Result {
    let indent = "  ".repeat(depth);
    match node {
        Node::Map(children) => {
            for (key, child) in children {
                write!(out, "{indent}{key}:")?;
                render_value(child, depth, out)?;
            }
        }
        Node::List { items, len } => {
            for item in items {
                write!(out, "{indent}-")?;
                render_value(item, depth, out)?;
            }
            if *len > items.len() {
                writeln!(out, "{indent}- ... {} more", len - items.len())?;
            }
        }
        Node::Scalar { text, mark } => {
            write!(out, "{indent}{text}")?;
            if let Some(mark) = mark {
                write!(out, "  ({mark})")?;
            }
            writeln!(out)?;
        }
    }
    Ok(())
}

/// Writes what follows a `key:` or `-` on its line and below.
fn render_value(node: &Node, depth: usize, out: &mut String) -> fmt::Result {
    match node {
        Node::Scalar { text, mark } => {
            write!(out, " {text}")?;
            if let Some(mark) = mark {
                write!(out, "  ({mark})")?;
            }
            writeln!(out)
        }
        Node::Map(children) if children.is_empty() => writeln!(out, " {{}}"),
        Node::Map(_) => {
            writeln!(out)?;
            render(node, depth + 1, out)
        }
        Node::List { len, .. } => {
            writeln!(out, " list of {len}")?;
            render(node, depth + 1, out)
        }
    }
}

/// A preset that cannot be read or written.
///
/// The message names what failed, the cause and the fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatError {
    /// What failed.
    pub what: String,
    /// Why, naming the key or offset where there is one.
    pub cause: String,
    /// How to fix it.
    pub fix: String,
}

impl FormatError {
    /// Builds an error from its three parts.
    pub fn new(what: impl Into<String>, cause: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            cause: cause.into(),
            fix: fix.into(),
        }
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}. {}", self.what, self.cause, self.fix)
    }
}

impl Error for FormatError {}

/// The format of a synth.
///
/// # Errors
///
/// [`SynthError::NotBuilt`] for a synth outside [`Synth::READY`].
pub fn format_for(synth: Synth) -> Result<&'static dyn PresetFormat, SynthError> {
    match synth.ready()? {
        Synth::Vital => Ok(&crate::preset::vital::FORMAT),
        Synth::Serum2 => Err(SynthError::NotBuilt(synth)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_renders_an_indented_tree() {
        let tree = Description {
            root: Node::Map(vec![
                ("name".into(), Node::scalar("\"Init\"")),
                (
                    "settings".into(),
                    Node::Map(vec![
                        (
                            "cutoff".into(),
                            Node::Scalar {
                                text: "200.0".into(),
                                mark: Some("out of range: 8.0 to 136.0".into()),
                            },
                        ),
                        ("empty".into(), Node::Map(Vec::new())),
                    ]),
                ),
                (
                    "slots".into(),
                    Node::List {
                        len: 5,
                        items: vec![
                            Node::Map(vec![("source".into(), Node::scalar("\"lfo_1\""))]),
                            Node::scalar("2"),
                        ],
                    },
                ),
            ]),
        };
        assert_eq!(
            tree.to_string(),
            "name: \"Init\"\n\
             settings:\n  cutoff: 200.0  (out of range: 8.0 to 136.0)\n  empty: {}\n\
             slots: list of 5\n  -\n    source: \"lfo_1\"\n  - 2\n  - ... 3 more\n"
        );
    }

    #[test]
    fn header_aligns_labels_and_marks_status() {
        let mut header = Header::default();
        header.push("build", "1.0.6", Some(Status::Ok));
        header.push("unknown keys", "2", Some(Status::Warn));
        assert_eq!(
            header.to_string(),
            "build         1.0.6\nunknown keys  2  [warn]\n"
        );
    }

    #[test]
    fn errors_print_three_parts() {
        let error = FormatError::new("cannot read x", "no `settings`", "Save it again");
        assert_eq!(
            error.to_string(),
            "cannot read x: no `settings`. Save it again"
        );
    }

    #[test]
    fn serum2_has_no_format_yet() {
        assert_eq!(
            format_for(Synth::Serum2).err(),
            Some(SynthError::NotBuilt(Synth::Serum2))
        );
    }
}
