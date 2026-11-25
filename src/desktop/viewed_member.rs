//! Preserve the path through the containers the reader actually opened. Names
//! are archive entries, never extraction paths or a lookup in the project.
use super::{input, native_preview, ArchiveFormat, Source, Viewer};
use std::{fs::File, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(in crate::desktop) struct ViewedMember {
    archive: String,
    original: String,
    steps: Vec<(ArchiveFormat, usize, String)>,
}

/// A history/listing link addresses an entry in the file being opened. A
/// selection transferred from another viewer additionally pins the outer bytes.
pub(in crate::desktop) enum MemberTarget {
    Zip(usize, String),
    Viewed(ViewedMember),
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

impl MemberTarget {
    pub(super) fn resolve(
        self,
        root: Source,
        mut remember: impl FnMut(&Source, &str),
    ) -> Result<Source, String> {
        match self {
            Self::Viewed(entry) => entry.resolve(root, remember),
            Self::Zip(index, name) => {
                remember(&root, &name);
                Ok(Source::Member {
                    container: Arc::new(root),
                    format: ArchiveFormat::Zip,
                    index,
                    name,
                })
            }
        }
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
    pub(in crate::desktop) fn check_archive(&self, digest: &str) -> Result<(), String> {
        if self.archive != digest {
            return Err(
                "The enclosing archive differs from the preview. Reopen the member.".into(),
            );
        }
        Ok(())
    }
    /// Inline rendering can start with the bytes of an issued ZIP entry. Join
    /// that local path to the issued file only if it is that exact snapshot.
    pub(in crate::desktop) fn in_issued_zip(
        mut self,
        inner: &str,
        archive: &str,
        original: &str,
        index: usize,
        name: &str,
    ) -> Result<Self, String> {
        self.check_archive(inner)?;
        self.steps
            .insert(0, (ArchiveFormat::Zip, index, name.to_owned()));
        self.archive = archive.to_owned();
        self.original = original.to_owned();
        Ok(self)
    }
    fn resolve(
        &self,
        mut source: Source,
        mut remember: impl FnMut(&Source, &str),
    ) -> Result<Source, String> {
        let Source::Snapshot { sha256, .. } = &source else {
            return Err("A retained archive is required.".into());
        };
        self.check_archive(sha256)?;
        for (format, index, name) in &self.steps {
            remember(&source, name);
            source = Source::Member {
                container: Arc::new(source),
                format: *format,
                index: *index,
                name: name.clone(),
            };
        }
        Ok(source)
    }
    pub(in crate::desktop) fn copy_from(
        &self,
        directory: Arc<tempfile::TempDir>,
        path: PathBuf,
        digest: String,
        output: &mut File,
    ) -> Result<(), String> {
        let source = self.resolve(
            Source::Snapshot {
                path,
                sha256: digest,
                _directory: directory,
            },
            |_, _| {},
        )?;
        let cancel = native_preview::Cancellation::default();
        input::copy(&mut source.open_input(&cancel)?, output, &cancel)
    }
}
