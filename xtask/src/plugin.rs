//! The Vital plug-in setup that the render tasks share: the bundle, the
//! host binary, renderers for a session and the load check before a batch.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use sound_to_patch::render::check::render_checked;
use sound_to_patch::render::plugin::{HOST_ENV, PluginRenderer, find_plugin, host_binary};
use sound_to_patch::render::recipe::Recipe;
use sound_to_patch::render::session::{BoxedRenderer, RenderOutcome, RenderSession};
use sound_to_patch::render::state::preset_state;
use sound_to_patch::render::{PresetState, RenderError, RenderJob, Rendered};
use sound_to_patch::synth::Synth;

use crate::Result;
use crate::fetch::Sha256Digest;

/// The plug-in bundle and the host binary.
#[derive(Clone, Debug)]
pub(crate) struct Plugin {
    bundle: PathBuf,
    host: PathBuf,
}

impl Plugin {
    /// Takes `bundle` and `host`, or finds them as `render` does.
    pub(crate) fn find(bundle: Option<&Path>, host: Option<&Path>) -> Result<Self> {
        let bundle = match bundle {
            Some(path) => path.to_owned(),
            None => {
                find_plugin(Synth::Vital).ok_or("cannot find the Vital plug-in; pass --plugin")?
            }
        };
        let host = match host {
            Some(path) => path.to_owned(),
            None => host_binary().ok_or_else(|| {
                format!("cannot find sound-to-patch-host; set {HOST_ENV} or pass --host")
            })?,
        };
        Ok(Self { bundle, host })
    }

    /// Returns the renderer factory of a session.
    pub(crate) fn spawn(
        &self,
    ) -> impl Fn(usize) -> std::result::Result<BoxedRenderer, RenderError> + Send + Sync + 'static
    {
        let Self { bundle, host } = self.clone();
        move |_worker| {
            PluginRenderer::start(Synth::Vital, &bundle, &host)
                .map(|renderer| Box::new(renderer) as BoxedRenderer)
        }
    }

    /// Runs the load check of `job` on a new renderer, so a batch never
    /// measures a preset the plug-in did not load. Prints the warning of an
    /// init-like render.
    pub(crate) fn check(&self, job: &RenderJob) -> Result<()> {
        let mut renderer = PluginRenderer::start(Synth::Vital, &self.bundle, &self.host)?;
        let (_, check) = render_checked(&mut renderer, job)?;
        if let Some(warning) = check.warning() {
            eprintln!("warning: {warning}");
        }
        Ok(())
    }

    /// Renders `job` `runs` times on a session of `workers` workers.
    pub(crate) fn render(
        &self,
        job: &RenderJob,
        runs: usize,
        workers: NonZeroUsize,
    ) -> Result<Vec<Rendered>> {
        let session = RenderSession::new(workers, self.spawn());
        session
            .render_all(&vec![job.clone(); runs])
            .into_iter()
            .map(|outcome| match outcome {
                RenderOutcome::Rendered(rendered) => Ok(rendered),
                RenderOutcome::Failed(failure) => Err(failure
                    .errors
                    .into_iter()
                    .last()
                    .map_or_else(|| "a render failed".into(), Into::into)),
            })
            .collect()
    }
}

/// Reads a Vital preset and builds its state. Returns the state and the
/// SHA-256 of the file.
pub(crate) fn read_preset(path: &Path) -> Result<(PresetState, String)> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let state = preset_state(Synth::Vital, &bytes, &Recipe::current())?;
    Ok((state, sha256_hex(&bytes)))
}

/// The SHA-256 of a file's bytes in lower-case hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256Digest(Sha256::digest(bytes).into()).to_string()
}
