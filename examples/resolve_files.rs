use derive_typst_intoval::{IntoDict, IntoValue};
use ecow::eco_format;
use std::fs;
use std::path::{Path, PathBuf};
use typst::diag::{FileError, FileResult, PackageError, PackageResult};
use typst::foundations::{Bytes, Dict, IntoValue, Smart};
use typst::syntax::package::PackageSpec;
use typst::syntax::FileId;
use typst::text::Font;
use typst_as_lib::TypstTemplate;

static TEMPLATE_FILE: &str = include_str!("./templates/resolve_files.typ");

static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

static PACKAGE_REPOSITORY_URL: &str = "https://packages.typst.org";

static DOWNLOAD_CACHE_DIR: &str = "./examples/download-cache";

static ROOT: &str = "./examples";

static OUTPUT: &str = "./examples/output.pdf";

fn main() {
    let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");

    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    #[allow(unused_mut)]
    let mut template = TypstTemplate::new(vec![font], TEMPLATE_FILE).file_resolver(resolve_files);
    let mut tracer = Default::default();

    // Run it
    let doc = template
        .compile(&mut tracer)
        .expect("typst::compile() returned an error!");

    // Create pdf
    let pdf = typst_pdf::pdf(&doc, Smart::Auto, None);
    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}

/// https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L95
fn resolve_files(id: FileId) -> FileResult<Bytes> {
    let path = if let Some(package) = id.package() {
        // Fetching file from package
        let package_dir = download_package(package)?;
        id.vpath().resolve(&package_dir)
    } else {
        // Fetching file from disk
        id.vpath().resolve(Path::new(ROOT))
    }
    .ok_or(FileError::AccessDenied)?;
    let content = std::fs::read(&path).map_err(|error| FileError::from_io(error, &path))?;
    Ok(content.into())
}

/// https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L115
/// Downloads the package and returns the system path of the unpacked package.
fn download_package(package: &PackageSpec) -> PackageResult<PathBuf> {
    let package_subdir = format!("{}/{}/{}", package.namespace, package.name, package.version);

    let path = Path::new(DOWNLOAD_CACHE_DIR).join(package_subdir);

    if path.exists() {
        return Ok(path);
    }

    eprintln!("downloading {package}");
    let url = format!(
        "{}/{}/{}-{}.tar.gz",
        PACKAGE_REPOSITORY_URL, package.namespace, package.name, package.version,
    );

    let response = retry(|| {
        let response = ureq::get(&url)
            .call()
            .map_err(|error| eco_format!("{error}"))?;

        let status = response.status();
        if !http_successful(status) {
            return Err(eco_format!(
                "response returned unsuccessful status code {status}",
            ));
        }

        Ok(response)
    })
    .map_err(|error| PackageError::NetworkFailed(Some(error)))?;

    let mut compressed_archive = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut compressed_archive)
        .map_err(|error| PackageError::NetworkFailed(Some(eco_format!("{error}"))))?;
    let raw_archive = zune_inflate::DeflateDecoder::new(&compressed_archive)
        .decode_gzip()
        .map_err(|error| PackageError::MalformedArchive(Some(eco_format!("{error}"))))?;
    let mut archive = tar::Archive::new(raw_archive.as_slice());
    archive.unpack(&path).map_err(|error| {
        _ = std::fs::remove_dir_all(&path);
        PackageError::MalformedArchive(Some(eco_format!("{error}")))
    })?;

    Ok(path)
}

fn retry<T, E>(mut f: impl FnMut() -> Result<T, E>) -> Result<T, E> {
    if let Ok(ok) = f() {
        Ok(ok)
    } else {
        f()
    }
}

fn http_successful(status: u16) -> bool {
    // 2XX
    status / 100 == 2
}
