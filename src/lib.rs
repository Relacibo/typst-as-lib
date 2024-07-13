use std::collections::HashMap;

use chrono::{Datelike, Duration, Local};
use comemo::Prehashed;
use typst::diag::{FileError, FileResult, SourceResult};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime, Dict};
use typst::model::Document;
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::Library;

// Inspired by https://github.com/tfachmann/typst-as-library/blob/main/src/lib.rs

#[derive(Debug, Clone)]
pub struct TypstTemplate {
    book: Prehashed<FontBook>,
    source: Source,
    other_sources: Option<HashMap<FileId, Source>>,
    files: Option<HashMap<FileId, Bytes>>,
    fonts: Vec<Font>,
}

impl TypstTemplate {
    /// Read in fonts and the main source file. It will have the id: `/template.typ`.
    pub fn new(fonts: Vec<Font>, source: String) -> Self {
        Self {
            book: Prehashed::new(FontBook::from_fonts(&fonts)),
            source: Source::new(FileId::new(None, VirtualPath::new("/template.typ")), source),
            fonts,
            other_sources: None,
            files: None,
        }
    }

    /// Set sources for template
    /// Example:
    /// ```rust
    /// static OTHER_SOURCE: &str = include_str!("./templates/other_source.typ");
    /// // ...
    /// let file_id = FileId::new(None, VirtualPath::new("/other_source.typ"))
    /// let other_sources: HashMap<FileId, String> =
    ///     std::iter::once((file_id, OTHER_SOURCE.to_owned())).collect();
    /// template = template.with_other_sources(other_sources);
    /// ```
    pub fn with_other_sources(self, other_sources: HashMap<FileId, String>) -> Self {
        Self {
            other_sources: Some(
                other_sources
                    .into_iter()
                    .map(|(id, s)| (id, Source::new(id, s)))
                    .collect(),
            ),
            ..self
        }
    }

    /// Set binary files for template
    /// Example:
    /// ```rust
    /// static IMAGE: &[u8] = include_bytes!("./images/image.png");
    /// // ...
    /// let file_id = FileId::new(None, VirtualPath::new("/images/image.png"))
    /// let other_files: HashMap<FileId, Bytes> =
    ///     std::iter::once((file_id, IMAGE.to_owned())).collect();
    /// template = template.with_other_binary_files(other_files);
    /// ```
    pub fn with_binary_files(self, files: HashMap<FileId, &[u8]>) -> Self {
        Self {
            files: Some(
                files
                    .into_iter()
                    .map(|(id, b)| (id, Bytes::from(b)))
                    .collect(),
            ),
            ..self
        }
    }

    /// Call `typst::compile()` with our template and a `Dict` as input, that will be availible 
    /// in a typst script with `#import sys: inputs`.
    pub fn compile_with_input(&self, tracer: &mut Tracer, input: Dict) -> SourceResult<Document> {
        let library = Prehashed::new(Library::builder().with_inputs(input).build());
        let world = TypstWorld {
            library,
            template: self,
        };
        typst::compile(&world, tracer)
    }

    /// Just call `typst::compile()`
    pub fn compile(&self, tracer: &mut Tracer) -> SourceResult<Document> {
        let library = Prehashed::new(Default::default());
        let world = TypstWorld {
            library,
            template: self,
        };
        typst::compile(&world, tracer)
    }
}

struct TypstWorld<'a> {
    library: Prehashed<Library>,
    template: &'a TypstTemplate,
}

impl typst::World for TypstWorld<'_> {
    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    fn book(&self) -> &Prehashed<FontBook> {
        &self.template.book
    }

    fn main(&self) -> Source {
        self.template.source.clone()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        fn get_file_helper(
            other_sources: &Option<HashMap<FileId, Source>>,
            id: FileId,
        ) -> Option<Source> {
            let other_sources = other_sources.as_ref()?;
            other_sources.get(&id).cloned()
        }

        let TypstWorld {
            template: TypstTemplate { other_sources, .. },
            ..
        } = self;

        if let Some(source) = get_file_helper(other_sources, id) {
            return Ok(source);
        }

        if id == self.main().id() {
            return Ok(self.main());
        }

        Err(FileError::NotFound(
            id.vpath().as_rooted_path().to_path_buf(),
        ))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        fn get_file_helper(
            other_files: &Option<HashMap<FileId, Bytes>>,
            id: FileId,
        ) -> Option<Bytes> {
            let other_files = other_files.as_ref()?;
            other_files.get(&id).cloned()
        }

        let TypstWorld {
            template: TypstTemplate {
                files: other_files, ..
            },
            ..
        } = self;

        if let Some(file) = get_file_helper(other_files, id) {
            return Ok(file);
        }

        Err(FileError::NotFound(
            id.vpath().as_rooted_path().to_path_buf(),
        ))
    }

    fn font(&self, id: usize) -> Option<Font> {
        self.template.fonts.get(id).cloned()
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        let mut now = Local::now();
        if let Some(offset) = offset {
            now += Duration::hours(offset);
        }
        let date = now.date_naive();
        let year = date.year();
        let month = (date.month0() + 1) as u8;
        let day = (date.day0() + 1) as u8;
        Datetime::from_ymd(year, month, day)
    }
}
