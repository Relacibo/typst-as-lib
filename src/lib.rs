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
    fonts: Vec<Font>,
}

impl TypstTemplate {
    pub fn new(fonts: Vec<Font>, source: String) -> Self {
        Self {
            book: Prehashed::new(FontBook::from_fonts(&fonts)),
            source: Source::new(FileId::new(None, VirtualPath::new("/template.typ")), source),
            fonts,
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
        let template = TypstTemplate::new(fonts, source);
        template.compile(tracer, content)
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

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        return Err(FileError::NotFound(
            id.vpath().as_rooted_path().to_path_buf(),
        ));
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
