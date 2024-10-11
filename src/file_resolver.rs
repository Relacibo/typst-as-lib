use ecow::eco_format;
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
};
use typst::{
    diag::{FileError, FileResult},
    foundations::Bytes,
    syntax::{FileId, Source},
};

use crate::{
    util::{bytes_to_source, not_found},
    FileIdNewType, SourceNewType,
};

// https://github.com/typst/typst/blob/16736feb13eec87eb9ca114deaeb4f7eeb7409d2/crates/typst-kit/src/package.rs#L18
/// The default packages sub directory within the package and package cache paths.
pub const DEFAULT_PACKAGES_SUBDIR: &str = "typst/packages";

pub trait FileResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>>;
    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>>;
}

#[derive(Debug, Clone)]
pub(crate) struct MainSourceFileResolver {
    main_source: Source,
}

impl MainSourceFileResolver {
    pub(crate) fn new(main_source: Source) -> Self {
        Self { main_source }
    }
}

impl FileResolver for MainSourceFileResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        Err(not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        let Self { main_source } = self;
        if id == main_source.id() {
            return Ok(Cow::Borrowed(main_source));
        }
        Err(not_found(id))
    }
}

#[derive(Debug, Clone)]
pub struct StaticSourceFileResolver {
    sources: HashMap<FileId, Source>,
}

impl StaticSourceFileResolver {
    pub(crate) fn new<IS, S>(sources: IS) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: Into<SourceNewType>,
    {
        let sources = sources
            .into_iter()
            .map(|s| {
                let SourceNewType(s) = s.into();
                (s.id(), s)
            })
            .collect();
        Self { sources }
    }
}

impl FileResolver for StaticSourceFileResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        Err(not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        self.sources
            .get(&id)
            .map(|s| Cow::Borrowed(s))
            .ok_or_else(|| not_found(id))
    }
}

#[derive(Debug, Clone)]
pub struct StaticFileResolver {
    binaries: HashMap<FileId, Bytes>,
}

impl StaticFileResolver {
    pub(crate) fn new<IB, F, B>(binaries: IB) -> Self
    where
        IB: IntoIterator<Item = (F, B)>,
        F: Into<FileIdNewType>,
        B: Into<Bytes>,
    {
        let binaries = binaries
            .into_iter()
            .map(|(id, b)| {
                let FileIdNewType(id) = id.into();
                (id, b.into())
            })
            .collect();
        Self { binaries }
    }
}

impl FileResolver for StaticFileResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        self.binaries
            .get(&id)
            .map(|b| Cow::Borrowed(b))
            .ok_or_else(|| not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        Err(not_found(id))
    }
}

#[derive(Debug, Clone)]
pub struct FileSystemResolver {
    root: PathBuf,
    local_package_root: Option<PathBuf>,
}

impl FileSystemResolver {
    pub fn new(root: PathBuf) -> Self {
        let mut root = root.clone();
        // trailing slash is necessary for resolve function, which is, what this 'hack' does
        // https://users.rust-lang.org/t/trailing-in-paths/43166/9
        root.push("");
        Self {
            root,
            local_package_root: None,
        }
    }

    /// Use other path to look for local packages
    pub fn with_local_package_root(self, path: PathBuf) -> Self {
        Self {
            local_package_root: Some(path),
            ..self
        }
    }
}

impl FileSystemResolver {
    fn resolve_bytes(&self, id: FileId) -> FileResult<Vec<u8>> {
        let Self {
            root,
            local_package_root,
        } = self;
        // https://github.com/typst/typst/blob/16736feb13eec87eb9ca114deaeb4f7eeb7409d2/crates/typst-kit/src/package.rs#L102C16-L102C38
        let dir: Cow<Path> = if let Some(package) = id.package() {
            let data_dir = if let Some(data_dir) = local_package_root {
                Cow::Borrowed(data_dir)
            } else if let Some(data_dir) = dirs::data_dir() {
                Cow::Owned(data_dir.join(DEFAULT_PACKAGES_SUBDIR))
            } else {
                return Err(FileError::Other(Some(eco_format!("No data dir set!"))));
            };
            let subdir = Path::new(package.namespace.as_str())
                .join(package.name.as_str())
                .join(package.version.to_string());
            Cow::Owned(data_dir.join(subdir))
        } else {
            Cow::Borrowed(root)
        };

        let path = id
            .vpath()
            .resolve(&dir)
            .ok_or_else(|| FileError::NotFound(dir.to_path_buf()))?;
        let content = std::fs::read(&path).map_err(|error| FileError::from_io(error, &path))?;
        Ok(content.into())
    }
}

impl FileResolver for FileSystemResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        let b = self.resolve_bytes(id)?;
        Ok(Cow::Owned(b.into()))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        let file = self.resolve_bytes(id)?;
        let source = bytes_to_source(id, &file)?;
        Ok(Cow::Owned(source))
    }
}
