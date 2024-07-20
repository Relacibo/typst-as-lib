use std::{borrow::Cow, collections::HashMap, path::PathBuf};
use typst::{
    diag::{FileError, FileResult},
    foundations::Bytes,
    syntax::{FileId, Source},
};

use crate::{
    util::{bytes_to_source, not_found},
    FileIdNewType, SourceNewType,
};

pub trait FileResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes>;
    fn resolve_source(&self, id: FileId) -> FileResult<Source>;
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
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes> {
        Err(not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Source> {
        let Self { main_source } = self;
        if id == main_source.id() {
            return Ok(main_source.clone());
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
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes> {
        Err(not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Source> {
        self.sources.get(&id).cloned().ok_or_else(|| not_found(id))
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
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes> {
        self.binaries.get(&id).cloned().ok_or_else(|| not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Source> {
        Err(not_found(id))
    }
}

#[derive(Debug, Clone)]
pub struct FileSystemResolver {
    root: PathBuf,
}

impl FileSystemResolver {
    pub fn new(root: PathBuf) -> Self {
        let mut root = root.clone();
        // trailing slash is necessary for resolve function, which is, what this 'hack' does
        // https://users.rust-lang.org/t/trailing-in-paths/43166/9
        root.push("");
        Self { root }
    }
}

impl FileSystemResolver {
    fn resolve_bytes(&self, id: FileId) -> FileResult<Vec<u8>> {
        if id.package().is_some() {
            return Err(not_found(id));
        }
        let Self { root } = self;

        let path = id.vpath().resolve(&root).ok_or(FileError::AccessDenied)?;
        let content = std::fs::read(&path).map_err(|error| FileError::from_io(error, &path))?;
        Ok(content.into())
    }
}

impl FileResolver for FileSystemResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes> {
        let b = self.resolve_bytes(id)?;
        Ok(b.into())
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Source> {
        let file = self.resolve_bytes(id)?;
        let source = bytes_to_source(id, &file)?;
        Ok(source)
    }
}
