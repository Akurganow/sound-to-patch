//! An in-memory adapter for the contract suites.
//!
//! Its file is text: a `build=<stamp>` line, then `key=<number>` lines and
//! an `end` line. Keys that start with `x_` are unknown to its registry.
//! Each [`Flaw`] breaks one rule of the format contract.

use std::any::Any;
use std::fmt::Write as _;
use std::path::PathBuf;

use sound_to_patch::preset::format::{
    DescribeOptions, Description, FormatError, Header, Node, PresetFormat, PresetModel, Route,
    Stored,
};
use sound_to_patch::preset::template::{Severity, TemplateCheck, TemplateHelp, TemplateSource};
use sound_to_patch::synth::Synth;

/// A deliberate defect of the in-memory format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flaw {
    None,
    DropsUnknownKeys,
    ChangesValues,
    PanicsOnEmpty,
}

#[derive(Debug)]
pub(crate) struct MemoryFormat {
    pub(crate) flaw: Flaw,
    pub(crate) template: MemoryTemplate,
}

impl MemoryFormat {
    pub(crate) fn new(flaw: Flaw) -> Self {
        Self {
            flaw,
            template: MemoryTemplate::default(),
        }
    }
}

/// A template source with settable candidates and built-in bytes.
#[derive(Clone, Debug, Default)]
pub(crate) struct MemoryTemplate {
    pub(crate) candidates: Vec<PathBuf>,
    pub(crate) builtin: Option<Vec<u8>>,
}

impl TemplateSource for MemoryTemplate {
    fn help(&self) -> TemplateHelp {
        TemplateHelp("Write a build=1 file and pass it with --template".to_owned())
    }

    fn candidates(&self) -> Vec<PathBuf> {
        self.candidates.clone()
    }

    fn builtin(&self) -> Option<Vec<u8>> {
        self.builtin.clone()
    }

    fn check(&self, model: &dyn PresetModel) -> Vec<TemplateCheck> {
        if model.get("level").is_some() {
            Vec::new()
        } else {
            vec![TemplateCheck {
                severity: Severity::Error,
                text: "no level".to_owned(),
                fix: "Add level=1".to_owned(),
            }]
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MemoryModel {
    build: String,
    values: Vec<(String, f64)>,
}

fn error(cause: &str) -> FormatError {
    FormatError::new(
        "cannot read the memory preset",
        cause,
        "Write build=, key= lines and end",
    )
}

impl PresetFormat for MemoryFormat {
    fn synth(&self) -> Synth {
        Synth::Vital
    }

    fn extension(&self) -> &'static str {
        "mem"
    }

    fn read(&self, bytes: &[u8]) -> Result<Box<dyn PresetModel>, FormatError> {
        assert!(
            !(bytes.is_empty() && self.flaw == Flaw::PanicsOnEmpty),
            "empty input"
        );
        let text = std::str::from_utf8(bytes).map_err(|_| error("not UTF-8"))?;
        if !text.ends_with("end\n") {
            return Err(error("no end line"));
        }
        let mut lines = text.lines();
        let build = lines
            .next()
            .and_then(|line| line.strip_prefix("build="))
            .ok_or_else(|| error("no build line"))?
            .to_owned();
        let mut values = Vec::new();
        for line in lines {
            if line == "end" {
                return Ok(Box::new(MemoryModel { build, values }));
            }
            let (key, value) = line.split_once('=').ok_or_else(|| error("no ="))?;
            let value = value.parse().map_err(|_| error("not a number"))?;
            values.push((key.to_owned(), value));
        }
        Err(error("no end line"))
    }

    fn read_header(&self, bytes: &[u8]) -> Result<Header, FormatError> {
        self.read(bytes).map(|model| model.header())
    }

    fn write(&self, model: &dyn PresetModel) -> Result<Vec<u8>, FormatError> {
        let model = model
            .as_any()
            .downcast_ref::<MemoryModel>()
            .ok_or_else(|| error("not a memory model"))?;
        let mut text = format!("build={}\n", model.build);
        for (key, value) in &model.values {
            if self.flaw == Flaw::DropsUnknownKeys && key.starts_with("x_") {
                continue;
            }
            let value = if self.flaw == Flaw::ChangesValues {
                value + 1.0
            } else {
                *value
            };
            writeln!(text, "{key}={value:?}").unwrap();
        }
        text.push_str("end\n");
        Ok(text.into_bytes())
    }

    fn absent_means_default(&self, _key: &str) -> bool {
        true
    }

    fn template(&self) -> &dyn TemplateSource {
        &self.template
    }
}

impl PresetModel for MemoryModel {
    fn header(&self) -> Header {
        let mut header = Header::default();
        header.push("build", self.build.clone(), None);
        header
    }

    fn get(&self, key: &str) -> Option<Stored> {
        self.values
            .iter()
            .find(|(have, _)| have == key)
            .map(|(_, value)| Stored::Float(*value))
    }

    fn set(&mut self, key: &str, value: Stored) -> Result<(), FormatError> {
        let Stored::Float(value) = value else {
            return Err(error("only floats"));
        };
        match self.values.iter_mut().find(|(have, _)| have == key) {
            Some(slot) => slot.1 = value,
            None => self.values.push((key.to_owned(), value)),
        }
        Ok(())
    }

    fn reset(&mut self, key: &str) {
        self.values.retain(|(have, _)| have != key);
    }

    fn routes(&self) -> Vec<Option<Route>> {
        Vec::new()
    }

    fn set_route(&mut self, _slot: usize, _route: Option<Route>) -> Result<(), FormatError> {
        Err(error("no slots"))
    }

    fn set_name(&mut self, _name: &str) -> Result<(), FormatError> {
        Ok(())
    }

    fn set_author(&mut self, _author: &str) -> Result<(), FormatError> {
        Ok(())
    }

    fn set_description(&mut self, _description: &str) -> Result<(), FormatError> {
        Ok(())
    }

    fn set_tags(&mut self, _tags: &[String]) -> Result<(), FormatError> {
        Ok(())
    }

    fn describe(&self, _opts: &DescribeOptions) -> Description {
        Description {
            root: Node::Map(
                self.values
                    .iter()
                    .map(|(key, value)| (key.clone(), Node::scalar(format!("{value:?}"))))
                    .collect(),
            ),
        }
    }

    fn unknown_keys(&self) -> Vec<String> {
        self.values
            .iter()
            .filter(|(key, _)| key.starts_with("x_"))
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
