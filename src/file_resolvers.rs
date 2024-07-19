use std::{
    cell::RefCell,
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
};

use binstall_tar::Archive;
use ecow::eco_format;
use flate2::read::GzDecoder;
use typst::{
    diag::{FileError, FileResult, PackageError, PackageResult},
    foundations::Bytes,
    syntax::{package::PackageSpec, FileId, Source, VirtualPath},
};

use crate::{
    util::{bytes_to_source, not_found},
    FileIdNewType, SourceNewType,
};

static PACKAGE_REPOSITORY_URL: &str = "https://packages.typst.org";

static REQUEST_RETRY_COUNT: u32 = 3;

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
        S: Into<SourceNewType>,
        IB: IntoIterator<Item = (F, B)>,
        F: Into<FileIdNewType>,
        B: Into<Bytes>,
    {
        let sources = sources
            .into_iter()
            .map(|s| {
                let SourceNewType(s) = s.into();
                (s.id(), s)
            })
            .collect();
        let binaries = binaries
            .into_iter()
            .map(|(id, b)| {
                let FileIdNewType(id) = id.into();
                (id, b.into())
            })
            .collect();
        Self { sources, binaries }
    }
}

impl FileResolver for StaticFileResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes> {
        self.binaries.get(&id).cloned().ok_or(not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Source> {
        self.sources.get(&id).cloned().ok_or(not_found(id))
    }
}

#[derive(Debug, Clone)]
pub struct FileSystemFileResolver {
    root: PathBuf,
}

impl FileSystemFileResolver {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl FileSystemFileResolver {
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

impl FileResolver for FileSystemFileResolver {
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

#[derive(Debug, Clone)]
pub struct PackageResolver {
    ureq: ureq::Agent,
    cache: Rc<RefCell<HashMap<FileId, Vec<u8>>>>,
}

impl PackageResolver {
    pub fn new(cache: Rc<RefCell<HashMap<FileId, Vec<u8>>>>, ureq: Option<ureq::Agent>) -> Self {
        let ureq = ureq.unwrap_or_else(|| ureq::Agent::new());
        Self { ureq, cache }
    }
}

impl PackageResolver {
    fn resolve_bytes<T>(&self, id: FileId) -> FileResult<T>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
    {
        let Self { ureq, cache } = self;
        let sobc = SourceOrBytesCreator;
        let Some(package) = id.package() else {
            return Err(not_found(id));
        };
        if let Some(res) = cache.as_ref().borrow().get(&id) {
            return sobc.try_create(id, res);
        }
        let PackageSpec {
            namespace,
            name,
            version,
        } = package;

        let url = format!(
            "{}/{}/{}-{}.tar.gz",
            PACKAGE_REPOSITORY_URL, namespace, name, version,
        );

        let mut last_error = eco_format!("");
        let mut response = None;
        for _ in 0..REQUEST_RETRY_COUNT {
            let resp = match ureq.get(&url).call() {
                Ok(resp) => resp,
                Err(error) => {
                    last_error = eco_format!("{error}");
                    continue;
                }
            };

            let status = resp.status();
            if status != 200 {
                last_error = eco_format!("response returned unsuccessful status code {status}",);
                continue;
            }
            response = Some(resp);
            break;
        }
        let response = response.ok_or_else(|| PackageError::NetworkFailed(Some(last_error)))?;

        let mut d = GzDecoder::new(response.into_reader());
        let mut archive = Vec::new();
        d.read_to_end(&mut archive)
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;

        let mut archive = Archive::new(&archive[..]);
        let entries = archive
            .entries()
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;
        let mut cache = cache.as_ref().borrow_mut();
        for entry in entries {
            let Ok(mut file) = entry else {
                continue;
            };
            let Ok(p) = file.path() else {
                continue;
            };
            let Some(file_name) = p.file_name() else {
                continue;
            };
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let file_id = FileId::new(Some(package.clone()), VirtualPath::new(file_name));
            let mut buf = Vec::new();
            let Ok(_) = file.read_to_end(&mut buf) else {
                continue;
            };
            cache.insert(file_id, buf);
        }

        let bytes = cache.get(&id).ok_or_else(|| not_found(id))?;
        sobc.try_create(id, bytes)
    }
}

impl FileResolver for PackageResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Bytes> {
        self.resolve_bytes(id)
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Source> {
        self.resolve_bytes(id)
    }
}

struct SourceOrBytesCreator;

trait CreateBytesOrSource<T> {
    fn try_create(&self, id: FileId, value: &[u8]) -> FileResult<T>;
}

impl CreateBytesOrSource<Source> for SourceOrBytesCreator {
    fn try_create(&self, id: FileId, value: &[u8]) -> FileResult<Source> {
        let source = bytes_to_source(id, value)?;
        Ok(source)
    }
}

impl CreateBytesOrSource<Bytes> for SourceOrBytesCreator {
    fn try_create(&self, _id: FileId, value: &[u8]) -> FileResult<Bytes> {
        Ok(Bytes::from(value))
    }
}
