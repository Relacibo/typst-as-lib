use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};

use binstall_tar::Archive;
use bytes::Buf;
use ecow::eco_format;
use flate2::read::GzDecoder;
use typst::{
    diag::{FileError, FileResult, PackageError},
    foundations::Bytes,
    syntax::{
        FileId, Source, SyntaxNode, VirtualPath,
        ast::{ModuleImport, Str},
        package::{PackageSpec, PackageVersion},
        parse,
    },
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

/// Builder for constructing a [`PackageResolver`].
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
    /// Creates a new builder.
    #[deprecated(since = "0.14.0", note = "Use `PackageResolver::builder()` instead")]
    pub fn new() -> PackageResolverBuilder<()> {
        PackageResolverBuilder::default()
    }

    /// Creates a new builder.
    #[deprecated(since = "0.14.1", note = "Use `PackageResolver::builder()` instead")]
    pub fn builder() -> PackageResolverBuilder<()> {
        PackageResolverBuilder::default()
    }
}

impl<C> PackageResolverBuilder<C> {
    /// Sets the number of retry attempts for failed HTTP requests.
    pub fn request_retry_count(mut self, request_retry_count: u32) -> Self {
        self.request_retry_count = Some(request_retry_count);
        self
    }

    /// Sets a custom `ureq` HTTP client.
    #[cfg(feature = "ureq")]
    pub fn ureq_agent(self, ureq: ureq::Agent) -> Self {
        Self {
            ureq: Some(ureq),
            ..self
        }
    }

    /// Sets a custom `reqwest` HTTP client.
    #[cfg(feature = "reqwest")]
    pub fn reqwest_client(self, reqwest: reqwest::blocking::Client) -> Self {
        Self {
            reqwest: Some(reqwest),
            ..self
        }
    }

    /// Sets a custom cache implementation.
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

    /// Uses the file system for caching packages.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::package_resolver::PackageResolver;
    /// let resolver = PackageResolver::builder()
    ///     .with_file_system_cache()
    ///     .build();
    /// ```
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

    /// Uses in-memory caching for packages.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::package_resolver::PackageResolver;
    /// let resolver = PackageResolver::builder()
    ///     .with_in_memory_cache()
    ///     .build();
    /// ```
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

    /// Builds the package resolver with the configured options.
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

/// Resolves and downloads packages from the Typst package repository.
#[derive(Debug, Clone)]
pub struct PackageResolver<C = ()> {
    #[cfg(feature = "ureq")]
    #[allow(dead_code)]
    ureq: ureq::Agent,
    #[cfg(feature = "reqwest")]
    #[allow(dead_code)]
    reqwest: reqwest::blocking::Client,
    cache: C,
    request_retry_count: u32,
}

impl PackageResolver {
    /// Creates a new builder for configuring a package resolver.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::package_resolver::PackageResolver;
    /// let resolver = PackageResolver::builder()
    ///     .with_file_system_cache()
    ///     .build();
    /// ```
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

        let url = Self::format_url_for_request(namespace, name, version);

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

    fn format_url_for_request(namespace: &str, name: &str, version: &PackageVersion) -> String {
        format!(
            "{}/{}/{}-{}.tar.gz",
            PACKAGE_REPOSITORY_URL, namespace, name, version,
        )
    }

    fn format_url_from_package_spec(package_spec: &PackageSpec) -> String {
        Self::format_url_for_request(
            &package_spec.namespace,
            &package_spec.name,
            &package_spec.version,
        )
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

/// File system cache for downloaded packages.
///
/// Uses the OS cache directory by default.
#[derive(Debug, Clone)]
pub struct FileSystemCache(pub PathBuf);

impl FileSystemCache {
    /// Creates a new file system cache with the default cache directory.
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

/// In-memory cache for downloaded packages.
#[derive(Debug, Clone, Default)]
pub struct InMemoryCache(pub Arc<Mutex<HashMap<FileId, Vec<u8>>>>);

impl InMemoryCache {
    /// Creates a new in-memory cache.
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

fn find_and_queue_packages(
    root: SyntaxNode,
    stack: &mut Vec<PackageSpec>,
    done: &HashSet<PackageSpec>,
) {
    let mut ast_stack: Vec<_> = vec![&root];
    let mut import_nodes = vec![];
    while let Some(node) = ast_stack.pop() {
        if let Some(_) = node.cast::<ModuleImport>() {
            for candidate in node.children() {
                if let Some(str_candidate) = candidate.cast::<Str>()
                    && str_candidate.get().starts_with("@preview")
                {
                    import_nodes.push(str_candidate);
                }
            }
        } else {
            ast_stack.extend(node.children());
        }
    }
    for import_name in import_nodes.into_iter().map(|import| import.get()) {
        let spec = PackageSpec::from_str(&import_name.as_str()).unwrap();
        if done.contains(&spec) || stack.contains(&spec) {
            continue;
        }
        stack.push(spec);
    }
}

/// Pre-populate the cache with package dependencies in an async context for a
/// given set of sources.
/// This doesn't pull in regular imports from the sources passed to it, so make
/// sure to pass all source files from a given template.
async fn async_prepopulate_dependencies<C: PackageResolverCache>(
    cache: &mut C,
    sources: impl IntoIterator<Item = Source>,
) -> FileResult<HashSet<PackageSpec>> {
    let mut packages_done: HashSet<PackageSpec> = HashSet::new();
    let mut files_done: HashSet<FileId> = HashSet::new();
    let mut packages_failed: HashSet<PackageSpec> = HashSet::new();
    let mut package_stack: Vec<PackageSpec> = vec![];
    let client = reqwest::ClientBuilder::new().build().unwrap();
    for source in sources {
        find_and_queue_packages(source.root().clone(), &mut package_stack, &packages_done);
    }
    while let Some(spec) = package_stack.pop() {
        if packages_done.contains(&spec) {
            continue;
        }
        let url = PackageResolver::<()>::format_url_from_package_spec(&spec);
        let mut archive_bytes = vec![];
        let Ok(response) = client.get(url).send().await else {
            packages_failed.insert(spec.clone());
            continue;
        };
        let Ok(bytes) = response.bytes().await else {
            packages_failed.insert(spec.clone());
            continue;
        };

        let mut decoder = GzDecoder::new(bytes.reader());
        decoder
            .read_to_end(&mut archive_bytes)
            .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;
        let mut archive = Archive::new(&archive_bytes[..]);
        let entries = archive.entries().unwrap();
        for (path, content) in entries.filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path().ok().map(|data| data.to_path_buf())?;
            let bytes: Vec<_> = entry.bytes().filter_map(|data| data.ok()).collect();
            let content = String::from_utf8(bytes).ok()?;
            Some((path, content))
        }) {
            let file_id = FileId::new(Some(spec.clone()), VirtualPath::new(path));
            if files_done.contains(&file_id) {
                continue;
            }
            let source = parse(&content);
            find_and_queue_packages(source, &mut package_stack, &packages_done);
            files_done.insert(file_id);
        }
        let Ok(_) = cache.cache_archive(Archive::new(&archive_bytes[..]), &spec) else {
            packages_failed.insert(spec);
            continue;
        };
        packages_done.insert(spec);
    }
    debug_assert!(packages_failed.len() == 0);
    Ok(packages_done)
}

#[cfg(test)]
mod test_async_packages {
    use std::iter;
    use tokio;

    use typst::syntax::{FileId, Source, VirtualPath};

    use crate::package_resolver::{PackageResolver, async_prepopulate_dependencies};

    const LOTS_OF_IMPORTS: &str = r#"
#import "@preview/cetz:0.5.2"
#import "@preview/fletcher:0.5.8"
#import "@preview/timeliney:0.4.0"
#import "@preview/pinit:0.2.2"
#import "@preview/deckz:0.3.1"
"#;
    #[tokio::test]
    async fn fetch_packages() {
        let source = Source::new(
            FileId::new(None, VirtualPath::new("/testing.typ")),
            LOTS_OF_IMPORTS.to_owned(),
        );
        let mut cache = PackageResolver::builder().with_in_memory_cache();
        let x = async_prepopulate_dependencies(&mut cache.cache, iter::once(source)).await;
        assert!(x.is_ok());
        let set = x.unwrap();
        dbg!(&set);
        assert!(set.len() > 0, "empty!");
    }
}
