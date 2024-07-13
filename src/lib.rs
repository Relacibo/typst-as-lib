use std::collections::HashMap;

use chrono::{Datelike, Duration, Local};
use comemo::Prehashed;
use typst::diag::{FileError, FileResult, SourceResult};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime, Dict};
use typst::model::Document;
use typst::syntax::package::PackageSpec;
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
    /// Initialize with fonts and a given source.
    /// - `source` can be of types:
    ///     - `String`, creating a detached Source
    ///     - `(&str, String)`, where &str is the absolute
    ///       virtual path of the Source file.
    ///     - `(FileId, String)`
    ///     - `typst::syntax::Source`
    pub fn new<S>(fonts: Vec<Font>, source: S) -> Self
    where
        S: Into<SourceNewType>,
    {
        let SourceNewType(source) = source.into();
        Self {
            book: Prehashed::new(FontBook::from_fonts(&fonts)),
            source,
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
    /// let source = ("/other_source.typ", OTHER_SOURCE.to_owned());
    /// template = template.add_other_sources([source]);
    /// ```
    pub fn add_other_sources<I, S>(mut self, other_sources: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SourceNotDetachedNewType>,
    {
        let new_other_sources = other_sources.into_iter().map(|s| {
            let SourceNotDetachedNewType(s) = s.into();
            (s.id(), s)
        });
        self.other_sources.extend(new_other_sources);
        self
    }

    /// Add binary files for template
    /// Example:
    /// ```rust
    /// static IMAGE: &[u8] = include_bytes!("./images/image.png");
    /// // ...
    /// let tuple = ("/images/image.png", IMAGE);
    /// template = template.add_binary_files([tuple]);
    /// ```
    pub fn add_binary_files<I, F, B>(mut self, files: I) -> Self
    where
        I: IntoIterator<Item = (F, B)>,
        F: Into<FileIdNewType>,
        B: Into<Bytes>,
    {
        let new_files = files.into_iter().map(|(id, b)| {
            let FileIdNewType(id) = id.into();
            (id, b.into())
        });
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

#[derive(Clone, Debug, Hash)]
pub struct FileIdNewType(FileId);

impl From<FileId> for FileIdNewType {
    fn from(value: FileId) -> Self {
        FileIdNewType(value)
    }
}

impl From<&str> for FileIdNewType {
    fn from(value: &str) -> Self {
        FileIdNewType(FileId::new(None, VirtualPath::new(value)))
    }
}

impl From<(PackageSpec, &str)> for FileIdNewType {
    fn from((p, id): (PackageSpec, &str)) -> Self {
        FileIdNewType(FileId::new(Some(p), VirtualPath::new(id)))
    }
}

#[derive(Clone, Debug, Hash)]
pub struct SourceNewType(Source);

impl From<Source> for SourceNewType {
    fn from(source: Source) -> Self {
        SourceNewType(source)
    }
}

impl From<(&str, String)> for SourceNewType {
    fn from((path, source): (&str, String)) -> Self {
        let id = FileId::new(None, VirtualPath::new(path));
        let source = Source::new(id, source);
        SourceNewType(source)
    }
}

impl From<(FileId, String)> for SourceNewType {
    fn from((id, source): (FileId, String)) -> Self {
        let source = Source::new(id, source);
        SourceNewType(source)
    }
}

impl From<String> for SourceNewType {
    fn from(source: String) -> Self {
        let source = Source::detached(source);
        SourceNewType(source)
    }
}

#[derive(Clone, Debug, Hash)]
pub struct SourceNotDetachedNewType(Source);

impl From<Source> for SourceNotDetachedNewType {
    fn from(source: Source) -> Self {
        SourceNotDetachedNewType(source)
    }
}

impl From<(&str, String)> for SourceNotDetachedNewType {
    fn from((path, source): (&str, String)) -> Self {
        let id = FileId::new(None, VirtualPath::new(path));
        let source = Source::new(id, source);
        SourceNotDetachedNewType(source)
    }
}

impl From<(FileId, String)> for SourceNotDetachedNewType {
    fn from((id, source): (FileId, String)) -> Self {
        let source = Source::new(id, source);
        SourceNotDetachedNewType(source)
    }
}
