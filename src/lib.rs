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
    other_sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
    fonts: Vec<Font>,
}

impl TypstTemplate {
    /// Read in fonts and the main source file. It will have the id: `/template.typ`.
    pub fn new<S>(fonts: Vec<Font>, source: S) -> Self
    where
        S: Into<String>,
    {
        Self {
            book: Prehashed::new(FontBook::from_fonts(&fonts)),
            source: Source::new(
                FileId::new(None, VirtualPath::new("/template.typ")),
                source.into(),
            ),
            fonts,
            other_sources: Default::default(),
            files: Default::default(),
        }
    }

    /// Add sources for template
    /// Example:
    /// ```rust
    /// static OTHER_SOURCE: &str = include_str!("./templates/other_source.typ");
    /// // ...
    /// let file_id = FileId::new(None, VirtualPath::new("/other_source.typ"))
    /// template = template.add_other_sources_from_strings([(file_id, OTHER_SOURCE)]);
    /// ```
    pub fn add_other_sources_from_strings<I, S>(mut self, other_sources: I) -> Self
    where
        I: IntoIterator<Item = (FileId, S)>,
        S: Into<String>,
    {
        let new_other_sources = other_sources
            .into_iter()
            .map(|(id, s)| (id, Source::new(id, s.into())));
        self.other_sources.extend(new_other_sources);
        self
    }

    /// Add sources for template
    /// Example:
    /// ```rust
    /// static OTHER_SOURCE: &str = include_str!("./templates/other_source.typ");
    /// // ...
    /// let file_id = FileId::new(None, VirtualPath::new("/other_source.typ"))
    /// template = template.add_other_sources_from_strings([Source::new(file_id, OTHER_SOURCE.into())]);
    /// ```
    pub fn add_other_sources<I, S>(mut self, other_sources: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Source>,
    {
        let new_other_sources = other_sources.into_iter().map(|s| {
            let source: Source = s.into();
            (source.id(), source)
        });
        self.other_sources.extend(new_other_sources);
        self
    }

    /// Add binary files for template
    /// Example:
    /// ```rust
    /// static IMAGE: &[u8] = include_bytes!("./images/image.png");
    /// // ...
    /// let file_id = FileId::new(None, VirtualPath::new("/images/image.png"))
    /// template = template.add_binary_files([(file_id, IMAGE)]);
    /// ```
    pub fn add_binary_files<'a, I, B>(mut self, files: I) -> Self
    where
        I: IntoIterator<Item = (FileId, B)>,
        B: Into<Bytes>,
    {
        let new_files = files.into_iter().map(|(id, b)| (id, b.into()));
        self.files.extend(new_files);
        self
    }

    /// Call `typst::compile()` with our template and a `Dict` as input, that will be availible
    /// in a typst script with `#import sys: inputs`.
    pub fn compile_with_input<D>(&self, tracer: &mut Tracer, input: D) -> SourceResult<Document>
    where
        D: Into<Dict>,
    {
        let library = Prehashed::new(Library::builder().with_inputs(input.into()).build());
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
        let TypstWorld {
            template: TypstTemplate { other_sources, .. },
            ..
        } = self;

        if let Some(source) = other_sources.get(&id).cloned() {
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
        let TypstWorld {
            template: TypstTemplate { files, .. },
            ..
        } = self;

        files
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.vpath().as_rooted_path().to_path_buf()))
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
