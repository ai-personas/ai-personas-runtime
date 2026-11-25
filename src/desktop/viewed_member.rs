//! An entry viewed inside a retained artifact, not a path on the host. Bind
//! every level of the selection to the original outer bytes, independent of
//! which file the operator opens while the save dialog is up.
use super::{input, native_preview, ArchiveFormat, Source, Viewer};
use std::{fs::File, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(in crate::desktop) struct ViewedMember {
    archive: String,
    original: String,
    steps: Vec<(ArchiveFormat, usize, String)>,
}

impl Viewer {
    pub(in crate::desktop) fn viewed_member(&self) -> Option<ViewedMember> {
        let mut source = self.source.as_ref()?;
        let mut steps = Vec::new();
        while let Source::Member {
            container,
            format,
            index,
            name,
        } = source
        {
            steps.push((*format, *index, name.clone()));
            source = container;
        }
        let Source::Snapshot { sha256, .. } = source else {
            return None;
        };
        if steps.is_empty() {
            return None;
        }
        steps.reverse();
        Some(ViewedMember {
            archive: sha256.clone(),
            original: source.name(),
            steps,
        })
    }
}

impl ViewedMember {
    pub(in crate::desktop) fn name(&self) -> &str {
        &self.steps.last().expect("member has at least one step").2
    }

    pub(in crate::desktop) fn original(&self) -> &str {
        &self.original
    }

    pub(in crate::desktop) fn paths(&self) -> impl Iterator<Item = &str> {
        self.steps.iter().map(|(_, _, name)| name.as_str())
    }

    pub(in crate::desktop) fn copy_from(
        &self,
        directory: Arc<tempfile::TempDir>,
        path: PathBuf,
        digest: String,
        output: &mut File,
    ) -> Result<(), String> {
        if self.archive != digest {
            return Err(
                "The enclosing archive differs from the preview. Reopen the member before saving."
                    .into(),
            );
        }
        let mut source = Source::Snapshot {
            path,
            sha256: digest,
            _directory: directory,
        };
        for (format, index, name) in &self.steps {
            source = Source::Member {
                container: Arc::new(source),
                format: *format,
                index: *index,
                name: name.clone(),
            };
        }
        // Read the member as data. Do not render it, follow links, use member
        // names as extraction paths, or export the enclosing file on failure.
        let cancel = native_preview::Cancellation::default();
        input::copy(&mut source.open_input(&cancel)?, output, &cancel)
    }
}
