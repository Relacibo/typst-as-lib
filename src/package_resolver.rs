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
    syntax::{FileId, Source, VirtualPath, package::PackageSpec},
};

use crate::{
    cached_file_resolver::{CachedFileResolver, IntoCachedFileResolver},
    file_resolver::{DEFAULT_PACKAGES_SUBDIR, FileResolver},
    util::{bytes_to_source, not_found},
};

// https://github.com/typst/typst/blob/16736feb13eec87eb9ca114deaeb4f7eeb7409d2/crates/typst-kit/src/package.rs#L15
/// The default Typst registry.
static PACKAGE_REPOSITORY_URL: &str = "https://packages.typst.org";

static REQUEST_RETRY_COUNT: u32 = 3;

#[derive(Debug, Clone, Default)]
pub struct PackageResolverBuilder<C = ()> {
    #[cfg(feature = "ureq")]
    ureq: Option<ureq::Agent>,
    #[cfg(feature = "reqwest")]
    reqwest: Option<reqwest::blocking::Client>,
    cache: C,
    request_retry_count: Option<u32>,
}

impl PackageResolverBuilder<()> {
    #[deprecated(since = "0.14.0", note = "Use `PackageResolver::builder()` instead")]
    pub fn new() -> PackageResolverBuilder<()> {
        PackageResolverBuilder::default()
    }

    #[deprecated(since = "0.14.1", note = "Use `PackageResolver::builder()` instead")]
    pub fn builder() -> PackageResolverBuilder<()> {
        PackageResolverBuilder::default()
    }
}

impl<C> PackageResolverBuilder<C> {
    pub fn request_retry_count(mut self, request_retry_count: u32) -> Self {
        self.request_retry_count = Some(request_retry_count);
        self
    }

    #[cfg(feature = "ureq")]
    pub fn ureq_agent(self, ureq: ureq::Agent) -> Self {
        Self {
            ureq: Some(ureq),
            ..self
        }
    }

    #[cfg(feature = "reqwest")]
    pub fn reqwest_client(self, reqwest: reqwest::blocking::Client) -> Self {
        Self {
            reqwest: Some(reqwest),
            ..self
        }
    }

    pub fn cache<C1>(self, cache: C1) -> PackageResolverBuilder<C1> {
        let Self {
            request_retry_count,
            #[cfg(feature = "ureq")]
            ureq,
            #[cfg(feature = "reqwest")]
            reqwest,
            ..
        } = self;
        PackageResolverBuilder {
            request_retry_count,
            #[cfg(feature = "ureq")]
            ureq,
            #[cfg(feature = "reqwest")]
            reqwest,
            cache,
        }
    }

    pub fn with_file_system_cache(self) -> PackageResolverBuilder<FileSystemCache> {
        let Self {
            request_retry_count,
            #[cfg(feature = "ureq")]
            ureq,
            #[cfg(feature = "reqwest")]
            reqwest,
            ..
        } = self;
        PackageResolverBuilder {
            request_retry_count,
            #[cfg(feature = "ureq")]
            ureq,
            #[cfg(feature = "reqwest")]
            reqwest,
            cache: FileSystemCache::new(),
        }
    }

    pub fn with_in_memory_cache(self) -> PackageResolverBuilder<InMemoryCache> {
        let Self {
            request_retry_count,
            #[cfg(feature = "ureq")]
            ureq,
            #[cfg(feature = "reqwest")]
            reqwest,
            ..
        } = self;
        PackageResolverBuilder {
            request_retry_count,
            #[cfg(feature = "ureq")]
            ureq,
            #[cfg(feature = "reqwest")]
            reqwest,
            cache: InMemoryCache::new(),
        }
    }

    pub fn build(self) -> PackageResolver<C> {
        let Self {
            request_retry_count,
            #[cfg(feature = "ureq")]
            ureq,
            #[cfg(feature = "reqwest")]
            reqwest,
            cache,
        } = self;
        PackageResolver {
            request_retry_count: request_retry_count.unwrap_or(REQUEST_RETRY_COUNT),
            #[cfg(feature = "ureq")]
            ureq: ureq.unwrap_or_else(ureq::Agent::new_with_defaults),
            #[cfg(feature = "reqwest")]
            reqwest: reqwest.unwrap_or_else(reqwest::blocking::Client::default),
            cache,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackageResolver<C = ()> {
    #[cfg(feature = "ureq")]
    ureq: ureq::Agent,
    #[cfg(feature = "reqwest")]
    reqwest: reqwest::blocking::Client,
    cache: C,
    request_retry_count: u32,
}

impl PackageResolver {
    pub fn builder() -> PackageResolverBuilder<()> {
        PackageResolverBuilder::default()
    }
}

impl<C> PackageResolver<C> {
    fn resolve_bytes<T>(&self, id: FileId) -> FileResult<T>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
        C: PackageResolverCache,
    {
        let Self {
            request_retry_count,
            cache,
            ..
        } = self;

        let Some(package) = id.package() else {
            return Err(not_found(id));
        };

        // https://github.com/typst/typst/blob/16736feb13eec87eb9ca114deaeb4f7eeb7409d2/crates/typst-kit/src/package.rs#L102C16-L102C38
        if package.namespace != "preview" {
            return Err(not_found(id));
        }

        if let Ok(Some(cached)) = cache.lookup_cached(package, id) {
            return Ok(cached);
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

        let mut reader = Err(PackageError::Other(None));
        for i in 0..*request_retry_count {
            reader = self.make_get_request(&url);
            match reader {
                Err(_) => eprintln!("Failed fetching {url} (try {})", i + 1),
                Ok(_) => break,
            }
        }

        let mut d = GzDecoder::new(reader?);
        let mut archive = Vec::new();
        d.read_to_end(&mut archive)
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;

        let archive = Archive::new(&archive[..]);
        cache.cache_archive(archive, package)?;
        cache
            .lookup_cached(package, id)
            .and_then(|f| f.ok_or_else(|| not_found(id)))
    }

    #[cfg(feature = "ureq")]
    fn make_get_request(&self, url: &str) -> Result<ureq::BodyReader<'static>, PackageError> {
        let Self { ureq, .. } = self;
        let resp = ureq
            .get(url)
            .call()
            .map_err(|err| PackageError::NetworkFailed(Some(eco_format!("{err}"))))?;

        let status = resp.status();
        if status != 200 {
            return Err(PackageError::NetworkFailed(Some(eco_format!(
                "response returned unsuccessful status code {status}"
            ))));
        }
        let (_, body) = resp.into_parts();
        Ok(body.into_reader())
    }

    #[cfg(all(not(feature = "ureq"), feature = "reqwest"))]
    fn make_get_request(
        &self,
        url: &str,
    ) -> Result<bytes::buf::Reader<bytes::Bytes>, PackageError> {
        use bytes::Buf;

        let Self { reqwest, .. } = self;
        let resp = reqwest
            .get(url)
            .send()
            .map_err(|err| PackageError::NetworkFailed(Some(eco_format!("{err}"))))?;

        let status = resp.status();
        if status != 200 {
            return Err(PackageError::NetworkFailed(Some(eco_format!(
                "response returned unsuccessful status code {status}"
            ))));
        }
        let bytes = resp
            .bytes()
            .map_err(|err| PackageError::NetworkFailed(Some(eco_format!("{err}"))))?;
        Ok(bytes.reader())
    }
}

impl<C> FileResolver for PackageResolver<C>
where
    C: PackageResolverCache,
{
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<'_, Bytes>> {
        let cached: Bytes = self.resolve_bytes(id)?;
        Ok(Cow::Owned(cached))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<'_, Source>> {
        let cached: Source = self.resolve_bytes(id)?;
        Ok(Cow::Owned(cached))
    }
}

fn compose_cache_file_path(root: &Path, package: &PackageSpec) -> FileResult<PathBuf> {
    let subdir = Path::new(package.namespace.as_str())
        .join(package.name.as_str())
        .join(package.version.to_string());

    Ok(root.join(subdir))
}

trait PackageResolverCache {
    fn lookup_cached<T>(&self, package: &PackageSpec, id: FileId) -> FileResult<Option<T>>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>;
    fn cache_archive(&self, archive: Archive<&[u8]>, package: &PackageSpec) -> FileResult<()>;
}

/// File system cache with given path
/// If content is None, then it uses <OS_CACHE_DIR>/typst/packages for caching.
#[derive(Debug, Clone)]
pub struct FileSystemCache(pub PathBuf);

impl FileSystemCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for FileSystemCache {
    fn default() -> Self {
        let cache_dir = dirs::cache_dir()
            .map(Cow::Owned)
            .unwrap_or_else(|| Cow::Borrowed(Path::new(".")));
        let path = cache_dir.join(DEFAULT_PACKAGES_SUBDIR);
        Self(path)
    }
}

impl PackageResolverCache for FileSystemCache {
    fn lookup_cached<T>(&self, package: &PackageSpec, id: FileId) -> FileResult<Option<T>>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
    {
        let FileSystemCache(path) = self;
        let dir = compose_cache_file_path(path, package)?;

        let Some(path) = id.vpath().resolve(&dir) else {
            return Ok(None);
        };
        let content = std::fs::read(&path).map_err(|error| FileError::from_io(error, &path))?;
        let cached = SourceOrBytesCreator.try_create(id, &content)?;
        Ok(Some(cached))
    }

    fn cache_archive(&self, mut archive: Archive<&[u8]>, package: &PackageSpec) -> FileResult<()> {
        let FileSystemCache(path) = self;
        let dir = compose_cache_file_path(path, package)?;
        std::fs::create_dir_all(&dir).map_err(|error| FileError::from_io(error, &dir))?;
        archive
            .unpack(&dir)
            .map_err(|error| FileError::from_io(error, &dir))?;
        Ok(())
    }
}

/// In memory cache
#[derive(Debug, Clone, Default)]
pub struct InMemoryCache(pub Arc<Mutex<HashMap<FileId, Vec<u8>>>>);

impl InMemoryCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PackageResolverCache for InMemoryCache {
    fn lookup_cached<T>(&self, _package: &PackageSpec, id: FileId) -> FileResult<Option<T>>
    where
        SourceOrBytesCreator: CreateBytesOrSource<T>,
    {
        let InMemoryCache(cache) = self;
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
        let InMemoryCache(cache) = self;
        let entries = archive
            .entries()
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;
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
            let mut mutex_guard = cache
                .lock()
                .map_err(|_| FileError::Other(Some(eco_format!("Could not lock cache"))))?;
            mutex_guard.insert(file_id, buf);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
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
        Ok(Bytes::new(value.to_vec()))
    }
}

impl IntoCachedFileResolver for PackageResolver<InMemoryCache> {
    fn into_cached(self) -> CachedFileResolver<Self> {
        CachedFileResolver::new(self).with_in_memory_source_cache()
    }
}

impl IntoCachedFileResolver for PackageResolver<FileSystemCache> {
    fn into_cached(self) -> CachedFileResolver<Self> {
        CachedFileResolver::new(self)
            .with_in_memory_source_cache()
            .with_in_memory_binary_cache()
    }
}
