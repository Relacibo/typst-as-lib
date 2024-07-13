use chrono::{Datelike, Duration, Local};
use comemo::Prehashed;
use typst::diag::{FileError, FileResult, SourceResult};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime, Dict};
use typst::model::Document;
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::Library;

/// Main interface that determines the environment for Typst.
#[derive(Debug, Clone)]
pub struct TypstTemplate {
    /// The standard library.
    book: Prehashed<FontBook>,

    source: Source,

    fonts: Vec<Font>,
}

impl TypstTemplate {
    pub fn new(source: String) -> Self {
        Self {
            book: Prehashed::new(FontBook::new()),
            source: Source::new(FileId::new(None, VirtualPath::new("/template.typ")), source),
            fonts: Default::default(),
        }
    }

    pub fn with_fonts(self, fonts: Vec<Font>) -> Self {
        Self {
            book: Prehashed::new(FontBook::from_fonts(&fonts)),
            fonts,
            ..self
        }
    }

    pub fn compile(&self, tracer: &mut Tracer, content: Dict) -> SourceResult<Document> {
        let library = Prehashed::new(Library::builder().with_inputs(content).build());
        let world = TypstWorld {
            library,
            template: self,
        };
        typst::compile(&world, tracer)
    }

    pub fn compile_directly(
        fonts: Vec<Font>,
        source: String,
        content: Dict,
        tracer: &mut Tracer,
    ) -> SourceResult<Document> {
        let template = TypstTemplate::new(source).with_fonts(fonts);
        template.compile(tracer, content)
    }
}

struct TypstWorld<'a> {
    library: Prehashed<Library>,
    template: &'a TypstTemplate,
}

/// This is the interface we have to implement such that `typst` can compile it.
///
/// I have tried to keep it as minimal as possible
impl typst::World for TypstWorld<'_> {
    /// Standard library.
    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    /// Metadata about all known Books.
    fn book(&self) -> &Prehashed<FontBook> {
        &self.template.book
    }

    /// Accessing the main source file.
    fn main(&self) -> Source {
        self.template.source.clone()
    }

    /// Accessing a specified source file (based on `FileId`).
    fn source(&self, id: FileId) -> FileResult<Source> {
        let package = id.package();
        let vpath = id
            .vpath()
            .as_rootless_path()
            .to_str()
            .ok_or(FileError::InvalidUtf8)?;
        let source = match (package, vpath) {
            (None, "template.typ") => self.template.source.clone(),
            _ => {
                return Err(FileError::NotFound(
                    id.vpath().as_rooted_path().to_path_buf(),
                ))
            }
        };
        Ok(source)
    }

    /// Accessing a specified file (non-file).
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        return Err(FileError::NotFound(
            id.vpath().as_rooted_path().to_path_buf(),
        ));
    }

    /// Accessing a specified font per index of font book.
    fn font(&self, id: usize) -> Option<Font> {
        self.template.fonts.get(id).cloned()
    }

    /// Get the current date.
    ///
    /// Optionally, an offset in hours is given.
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
