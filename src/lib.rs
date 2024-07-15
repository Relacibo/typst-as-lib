use std::collections::HashMap;

use chrono::{Datelike, Duration, Local};
use comemo::Prehashed;
use typst::diag::{FileError, FileResult, SourceResult};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime, Dict, Module, Scope};
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
    inject_location: Option<InjectLocation>,
}

#[derive(Debug, Clone)]
struct InjectLocation {
    preinitialized_library: Library,
    module_name: String,
    value_name: String,
}

impl TypstTemplate {
    /// Initialize with fonts and a given source.
    /// - `source` can be of types:
    ///     - `&str/String`, creating a detached Source (Has vpath `/main.typ`)
    ///     - `(&str, &str/String)`, where &str is the absolute
    ///       virtual path of the Source file.
    ///     - `(typst::syntax::FileId, &str/String)`
    ///     - `typst::syntax::Source`
    ///
    /// (`&str/String` is always the template file content)
    ///
    /// Example:
    /// ```rust
    /// static TEMPLATE: &str = include_str!("./templates/template.typ");
    /// static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
    /// // ...
    /// let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");
    /// let template = TypstTemplate::new(vec![font], TEMPLATE);
    /// ```
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
            inject_location: Default::default(),
        }
    }

    /// Initialize with fonts and string that will be converted to a source.
    /// It will have the virtual path: `/template.typ`.
    #[deprecated = "Use TypstTemplate::new instead"]
    pub fn new_from_string<S>(fonts: Vec<Font>, source: S) -> Self
    where
        S: Into<String>,
    {
        Self::new(fonts, source.into())
    }

    /// Add sources for template
    /// - `other_sources` The item of the IntoIterator can be of types:
    ///     - `&str/String`, creating a detached Source (Has vpath `/main.typ`)
    ///     - `(&str, &str/String)`, where &str is the absolute
    ///       virtual path of the Source file.
    ///     - `(typst::syntax::FileId, &str/String)`
    ///     - `typst::syntax::Source`
    ///
    /// (`&str/String` is always the template file content)
    ///
    /// Example:
    /// ```rust
    /// static OTHER_SOURCE: &str = include_str!("./templates/other_source.typ");
    /// // ...
    /// let source = ("/other_source.typ", OTHER_SOURCE);
    /// template = template.add_other_sources([source]);
    /// ```
    pub fn add_other_sources<I, S>(mut self, other_sources: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SourceNewType>,
    {
        let new_other_sources = other_sources.into_iter().map(|s| {
            let SourceNewType(s) = s.into();
            (s.id(), s)
        });
        self.other_sources.extend(new_other_sources);
        self
    }

    /// Add sources for template
    /// Example:
    /// ```rust
    /// static OTHER_SOURCE: &str = include_str!("./templates/other_source.typ");
    /// // ...
    /// let file_id = FileId::new(None, VirtualPath::new("/other_source.typ"))
    /// let tuple = (file_id, OTHER_SOURCE);
    /// template = template.add_other_sources_from_strings([tuple]);
    /// ```
    #[deprecated = "Use TypstTemplate::add_other_sources instead"]
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

    /// Replace main source
    #[deprecated = "Use TypstTemplate::source instead"]
    pub fn set_source<S>(self, source: S) -> Self
    where
        S: Into<SourceNewType>,
    {
        self.source(source)
    }

    /// Replace main source
    pub fn source<S>(self, source: S) -> Self
    where
        S: Into<SourceNewType>,
    {
        let SourceNewType(source) = source.into();
        Self { source, ..self }
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    /// Also preinitializes the library for better performance,
    /// if the template will be reused.
    /// TypstTemplate::compile will panic in debug build,
    /// if the location is already used.
    pub fn custom_inject_location<S>(self, module_name: S, value_name: S) -> Self
    where
        S: Into<String>,
    {
        Self {
            inject_location: Some(InjectLocation {
                preinitialized_library: Default::default(),
                module_name: module_name.into(),
                value_name: value_name.into(),
            }),
            ..self
        }
    }

    /// Add Fonts
    pub fn add_fonts<I, F>(mut self, fonts: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        let fonts = fonts.into_iter().map(Into::into);
        self.fonts.extend(fonts);
        self
    }

    /// Call `typst::compile()` with our template and a `Dict` as input, that will be availible
    /// in a typst script with `#import sys: inputs`.
    pub fn compile_with_input<D>(&self, tracer: &mut Tracer, inputs: D) -> SourceResult<Document>
    where
        D: Into<Dict>,
    {
        let Self {
            inject_location, ..
        } = self;
        let inputs = inputs.into();
        let library = if let Some(InjectLocation {
            preinitialized_library,
            module_name,
            value_name,
        }) = inject_location
        {
            let mut lib = preinitialized_library.clone();
            let global = lib.global.scope_mut();
            let mut scope = Scope::new();
            scope.define(value_name, inputs);
            let module = Module::new(module_name, scope);
            global.define_module(module);
            lib
        } else {
            Library::builder().with_inputs(inputs).build()
        };
        let world = TypstWorld {
            library: Prehashed::new(library),
            template: self,
        };
        typst::compile(&world, tracer)
    }

    /// Just call `typst::compile()`
    pub fn compile(&self, tracer: &mut Tracer) -> SourceResult<Document> {
        let world = TypstWorld {
            library: Default::default(),
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

impl From<FileIdNewType> for FileId {
    fn from(file_id: FileIdNewType) -> Self {
        let FileIdNewType(file_id) = file_id;
        file_id
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

impl From<SourceNewType> for Source {
    fn from(source: SourceNewType) -> Self {
        let SourceNewType(source) = source;
        source
    }
}

impl From<(&str, String)> for SourceNewType {
    fn from((path, source): (&str, String)) -> Self {
        let id = FileId::new(None, VirtualPath::new(path));
        let source = Source::new(id, source);
        SourceNewType(source)
    }
}

impl From<(&str, &str)> for SourceNewType {
    fn from((path, source): (&str, &str)) -> Self {
        SourceNewType::from((path, source.to_owned()))
    }
}

impl From<(FileId, String)> for SourceNewType {
    fn from((id, source): (FileId, String)) -> Self {
        let source = Source::new(id, source);
        SourceNewType(source)
    }
}

impl From<(FileId, &str)> for SourceNewType {
    fn from((id, source): (FileId, &str)) -> Self {
        SourceNewType::from((id, source.to_owned()))
    }
}

impl From<String> for SourceNewType {
    fn from(source: String) -> Self {
        let source = Source::detached(source);
        SourceNewType(source)
    }
}

impl From<&str> for SourceNewType {
    fn from(source: &str) -> Self {
        SourceNewType::from(source.to_owned())
    }
}
