use typst::{
    diag::{FileError, FileResult},
    syntax::{FileId, Source},
};

pub(crate) fn not_found(id: FileId) -> FileError {
    FileError::NotFound(id.vpath().as_rooted_path().to_path_buf())
}

pub(crate) fn bytes_to_source(id: FileId, bytes: &[u8]) -> FileResult<Source> {
    // https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L78
    let contents = std::str::from_utf8(bytes).map_err(|_| FileError::InvalidUtf8)?;
    let contents = contents.trim_start_matches('\u{feff}');
    Ok(Source::new(id, contents.to_owned()))
}
