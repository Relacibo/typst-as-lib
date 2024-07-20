use std::{borrow::Cow, collections::HashMap, path::PathBuf};
use typst::{
    diag::{FileError, FileResult},
    foundations::Bytes,
    syntax::{FileId, Source},
};

use crate::{
    util::{bytes_to_source, not_found},
    IntoBytes, IntoFileId, IntoSource,
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
pub struct StaticFileResolver {
    sources: HashMap<FileId, Source>,
    binaries: HashMap<FileId, Bytes>,
}

impl StaticFileResolver {
    pub(crate) fn new<IS, S, IB, F, B>(sources: IS, binaries: IB) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: IntoSource,
        IB: IntoIterator<Item = (F, B)>,
        F: IntoFileId,
        B: IntoBytes,
    {
        let mut collected_sources = HashMap::new();
        for source in sources.into_iter() {
            let source: Source = source.into_source(Default::default());
            collected_sources.insert(source.id(), source);
        }
        let mut collected_binaries = HashMap::new();
        for (file_id, binary) in binaries.into_iter() {
            let file_id = file_id.into_file_id(Default::default());
            let binary = binary.into_bytes();
            collected_binaries.insert(file_id, binary);
        }

        Self {
            sources: collected_sources,
            binaries: collected_binaries,
        }
    }
}

impl FileResolver for StaticFileResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes> {
        self.binaries.get(&id).cloned().ok_or_else(|| not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Source> {
        self.sources.get(&id).cloned().ok_or_else(|| not_found(id))
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
