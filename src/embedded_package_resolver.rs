use std::borrow::Cow;
use std::collections::HashMap;
use typst::diag::FileResult;
use typst::foundations::Bytes;
use typst::syntax::{FileId, Source};

use crate::file_resolver::FileResolver;
use crate::util::{bytes_to_source, not_found};

/// FileResolver that serves packages embedded at compile time.
///
/// Packages downloaded by build.rs are embedded using the `include_dir!` macro,
/// providing zero-overhead file resolution without filesystem access at runtime.
///
/// # Example
///
/// ```rust,ignore
/// let engine = TypstEngine::builder()
///     .main_file(template)
///     .fonts([font])
///     .with_bundled_packages()
///     .build();
/// ```
#[derive(Debug)]
pub struct EmbeddedPackageResolver {
    files: HashMap<String, &'static [u8]>,
}

impl EmbeddedPackageResolver {
    /// Create resolver from embedded packages directory
    pub fn new() -> Self {
        use include_dir::{Dir, include_dir};

        // Use environment variable set by build.rs
        // CRITICAL: env!() is evaluated at compile time
        static PACKAGES: Dir<'static> = include_dir!("$TYPST_BUNDLED_PACKAGES_DIR");

        let mut files = HashMap::new();
        collect_files(&PACKAGES, &mut files);

        Self { files }
    }

    /// Convert FileId to embedded file path.
    ///
    /// Uses same path convention as PackageResolver:
    /// {namespace}/{name}/{version}/{vpath}
    fn file_path(&self, id: FileId) -> String {
        if let Some(pkg) = id.package() {
            format!(
                "{}/{}/{}/{}",
                pkg.namespace.as_str(),
                pkg.name.as_str(),
                pkg.version,
                id.vpath().as_rootless_path().display()
            )
        } else {
            id.vpath().as_rootless_path().display().to_string()
        }
    }
}

impl Default for EmbeddedPackageResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl FileResolver for EmbeddedPackageResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<'_, Bytes>> {
        let path = self.file_path(id);

        self.files
            .get(&path)
            .map(|&bytes| Cow::Owned(Bytes::new(bytes)))
            .ok_or_else(|| not_found(id))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<'_, Source>> {
        let path = self.file_path(id);
        let bytes = self.files.get(&path).ok_or_else(|| not_found(id))?;
        let source = bytes_to_source(id, bytes)?;
        Ok(Cow::Owned(source))
    }
}

/// Recursively traverse include_dir's Dir to build HashMap
fn collect_files(dir: &'static include_dir::Dir, map: &mut HashMap<String, &'static [u8]>) {
    // file.path() returns full relative path from root
    for file in dir.files() {
        let path = file.path().display().to_string().replace('\\', "/");
        map.insert(path, file.contents());
    }

    // Recursively process subdirectories
    for subdir in dir.dirs() {
        collect_files(subdir, map);
    }
}
