use std::{
    borrow::Cow,
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use binstall_tar::Archive;
use ecow::eco_format;
use flate2::read::GzDecoder;
use typst::{
    diag::{FileError, FileResult, PackageError},
    foundations::Bytes,
    syntax::{package::PackageSpec, FileId, Source, VirtualPath},
};

use crate::{
    file_resolver::{FileResolver, DEFAULT_PACKAGES_SUBDIR},
    util::{bytes_to_source, not_found},
};

// https://github.com/typst/typst/blob/16736feb13eec87eb9ca114deaeb4f7eeb7409d2/crates/typst-kit/src/package.rs#L15
/// The default Typst registry.
static PACKAGE_REPOSITORY_URL: &str = "https://packages.typst.org";

static REQUEST_RETRY_COUNT: u32 = 3;

#[derive(Debug, Clone)]
pub struct PackageResolver<C> {
    ureq: ureq::Agent,
    cache: C,
}

impl<C> PackageResolver<C> {
    pub fn new(cache: C, ureq: Option<ureq::Agent>) -> Self {
        let ureq = ureq.unwrap_or_else(|| ureq::Agent::new());
        Self { ureq, cache }
    }
}

impl<C> PackageResolver<C> {
    fn resolve_bytes<T>(&self, id: FileId) -> FileResult<T>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
        C: PackageResolverBaseCache,
    {
        let Self { ureq, cache } = self;
        let Some(package) = id.package() else {
            return Err(not_found(id));
        };

        // https://github.com/typst/typst/blob/16736feb13eec87eb9ca114deaeb4f7eeb7409d2/crates/typst-kit/src/package.rs#L102C16-L102C38
        if package.namespace != "preview" {
            return Err(not_found(id));
        }

        match cache.lookup_cached(package, id) {
            Ok(Some(cached)) => return Ok(cached),
            _ => (),
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
                last_error = eco_format!("response returned unsuccessful status code {status}");
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

        let archive = Archive::new(&archive[..]);
        cache.cache_archive(archive, package)?;
        cache
            .lookup_cached(package, id)
            .and_then(|f| f.ok_or_else(|| not_found(id)))
    }
}

impl<C> FileResolver for PackageResolver<C>
where
    C: PackageResolverBaseCache,
{
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<Bytes>> {
        self.resolve_bytes(id).map(|b| Cow::Owned(b))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<Source>> {
        self.resolve_bytes(id).map(|s| Cow::Owned(s))
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

fn get_cache_file_path(path: Option<&Path>, package: &PackageSpec) -> FileResult<PathBuf> {
    let root = if let Some(path) = path {
        Cow::Borrowed(path)
    } else {
        let Some(cache_dir) = dirs::cache_dir() else {
            return Err(FileError::Other(Some(eco_format!("No cache dir set!"))));
        };
        Cow::Owned(cache_dir.join(DEFAULT_PACKAGES_SUBDIR))
    };
    let subdir = Path::new(package.namespace.as_str())
        .join(package.name.as_str())
        .join(package.version.to_string());

    Ok(root.join(subdir))
}

#[derive(Clone, Debug)]
pub enum PackageResolverCache {
    FileSystem(Option<PathBuf>),
    Memory(Arc<Mutex<HashMap<FileId, Vec<u8>>>>),
}

trait PackageResolverBaseCache {
    fn lookup_cached<T>(&self, package: &PackageSpec, id: FileId) -> FileResult<Option<T>>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>;
    fn cache_archive(&self, archive: Archive<&[u8]>, package: &PackageSpec) -> FileResult<()>;
}

/// File system cache with given path
/// If content is None, then it uses <OS_CACHE_DIR>/typst/packages for caching.
pub struct FileSystemBaseCache(Option<PathBuf>);

impl PackageResolverBaseCache for FileSystemBaseCache {
    fn lookup_cached<T>(&self, package: &PackageSpec, id: FileId) -> FileResult<Option<T>>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
    {
        let FileSystemBaseCache(path) = self;
        let dir = get_cache_file_path(path.as_deref(), package)?;

        let Some(path) = id.vpath().resolve(&dir) else {
            return Ok(None);
        };
        let content = std::fs::read(&path).map_err(|error| FileError::from_io(error, &path))?;
        let cached = SourceOrBytesCreator.try_create(id, &content)?;
        Ok(Some(cached))
    }

    fn cache_archive(&self, mut archive: Archive<&[u8]>, package: &PackageSpec) -> FileResult<()> {
        let FileSystemBaseCache(path) = self;
        let dir = get_cache_file_path(path.as_deref(), package)?;
        std::fs::create_dir_all(&dir).map_err(|error| FileError::from_io(error, &dir))?;
        archive
            .unpack(&dir)
            .map_err(|error| FileError::from_io(error, &dir))?;
        Ok(())
    }
}

/// In memory cache
pub struct MemoryBaseCache(Arc<Mutex<HashMap<FileId, Vec<u8>>>>);

impl PackageResolverBaseCache for MemoryBaseCache {
    fn lookup_cached<T>(&self, package: &PackageSpec, id: FileId) -> FileResult<Option<T>>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
    {
        let MemoryBaseCache(cache) = self;
        let mutex_guard = cache
            .as_ref()
            .lock()
            .map_err(|_| FileError::Other(Some(eco_format!("Could not lock cache"))))?;
        let cached = if let Some(value) = mutex_guard.get(&id) {
            let cached = SourceOrBytesCreator.try_create(id, value)?;
            Some(cached)
        } else {
            None
        };
        Ok(cached)
    }

    fn cache_archive(&self, mut archive: Archive<&[u8]>, package: &PackageSpec) -> FileResult<()> {
        let MemoryBaseCache(cache) = self;
        let entries = archive
            .entries()
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;
        let mut mutex_guard = cache
            .as_ref()
            .lock()
            .map_err(|_| FileError::Other(Some(eco_format!("Could not lock cache"))))?;
        for entry in entries {
            let Ok(mut file) = entry else {
                continue;
            };
            let Ok(p) = file.path() else {
                continue;
            };
            let file_id = FileId::new(Some(package.clone()), VirtualPath::new(p));
            let mut buf = Vec::new();
            let Ok(_) = file.read_to_end(&mut buf) else {
                continue;
            };
            mutex_guard.insert(file_id, buf);
        }
        Ok(())
    }
}

impl Default for PackageResolverCache {
    fn default() -> Self {
        PackageResolverCache::FileSystem(None)
    }
}
