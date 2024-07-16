use std::collections::HashMap;

use chrono::{Datelike, Duration, Local};
use comemo::Prehashed;
use ecow::{eco_vec, EcoVec};
use thiserror::Error;
use typst::diag::{FileError, FileResult, SourceDiagnostic, SourceResult};
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
    source: Source,
    collection: TypstTemplateCollection,
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
    pub fn new<V, S>(fonts: V, source: S) -> Self
    where
        V: Into<Vec<Font>>,
        S: Into<SourceNewType>,
    {
        let collection = TypstTemplateCollection::new(fonts);
        let SourceNewType(source) = source.into();
        Self { collection, source }
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
    pub fn add_other_sources<I, S>(self, other_sources: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SourceNewType>,
    {
        Self {
            collection: self.collection.add_sources(other_sources),
            ..self
        }
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
    pub fn add_other_sources_from_strings<I, S>(self, other_sources: I) -> Self
    where
        I: IntoIterator<Item = (FileId, S)>,
        S: Into<String>,
    {
        Self {
            collection: self
                .collection
                .add_sources(other_sources.into_iter().map(|(id, s)| (id, s.into()))),
            ..self
        }
    }

    /// Add binary files for template
    /// Example:
    /// ```rust
    /// static IMAGE: &[u8] = include_bytes!("./images/image.png");
    /// // ...
    /// let tuple = ("/images/image.png", IMAGE);
    /// template = template.add_binary_files([tuple]);
    /// ```
    pub fn add_binary_files<I, F, B>(self, files: I) -> Self
    where
        I: IntoIterator<Item = (F, B)>,
        F: Into<FileIdNewType>,
        B: Into<Bytes>,
    {
        Self {
            collection: self.collection.add_binary_files(files),
            ..self
        }
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
            collection: self
                .collection
                .custom_inject_location(module_name, value_name),
            ..self
        }
    }

    /// Add Fonts
    pub fn add_fonts<I, F>(self, fonts: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        Self {
            collection: self.collection.add_fonts(fonts),
            ..self
        }
    }

    /// Call `typst::compile()` with our template and a `Dict` as input, that will be availible
    /// in a typst script with `#import sys: inputs`.
    pub fn compile_with_input<D>(&self, tracer: &mut Tracer, inputs: D) -> SourceResult<Document>
    where
        D: Into<Dict>,
    {
        let Self {
            source, collection, ..
        } = self;
        let library = initialize_library(collection, inputs);
        let world = TypstWorld {
            library: Prehashed::new(library),
            collection,
            main_source: &source,
        };
        typst::compile(&world, tracer)
    }

    /// Just call `typst::compile()`
    pub fn compile(&self, tracer: &mut Tracer) -> SourceResult<Document> {
        let Self {
            source, collection, ..
        } = self;
        let world = TypstWorld {
            library: Default::default(),
            collection,
            main_source: source,
        };
        typst::compile(&world, tracer)
    }
}

#[derive(Debug, Clone)]
pub struct TypstTemplateCollection {
    book: Prehashed<FontBook>,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
    fonts: Vec<Font>,
    inject_location: Option<InjectLocation>,
}

impl TypstTemplateCollection {
    /// Initialize with fonts.
    ///
    /// Example:
    /// ```rust
    /// static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
    /// // ...
    /// let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");
    /// let template = TypstTemplate::new(vec![font]);
    /// ```
    pub fn new<V>(fonts: V) -> Self
    where
        V: Into<Vec<Font>>,
    {
        let fonts = fonts.into();
        Self {
            book: Prehashed::new(FontBook::from_fonts(&fonts)),
            fonts,
            sources: Default::default(),
            files: Default::default(),
            inject_location: Default::default(),
        }
    }

    /// Add sources for template
    /// - `sources` The item of the IntoIterator can be of types:
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
    /// static SOURCES: &str = include_str!("./templates/source.typ");
    /// // ...
    /// let source = ("/source.typ", SOURCES);
    /// template = template.add_sources([source]);
    /// ```
    pub fn add_sources<I, S>(mut self, sources: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SourceNewType>,
    {
        let new_sources = sources.into_iter().map(|s| {
            let SourceNewType(s) = s.into();
            (s.id(), s)
        });
        self.sources.extend(new_sources);
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
    pub fn compile_with_input<F, D>(
        &self,
        tracer: &mut Tracer,
        main_source: F,
        inputs: D,
    ) -> Result<Document, TypstAsLibError>
    where
        F: Into<FileIdNewType>,
        D: Into<Dict>,
    {
        let Self { sources, .. } = self;
        let FileIdNewType(main_source) = main_source.into();
        let main_source = sources
            .get(&main_source)
            .ok_or_else(|| TypstAsLibError::MainSourceFileDoesNotExist(main_source))?;
        let library = initialize_library(self, inputs);
        let world = TypstWorld {
            library: Prehashed::new(library),
            collection: self,
            main_source,
        };
        let doc = typst::compile(&world, tracer)?;
        Ok(doc)
    }

    /// Just call `typst::compile()`
    pub fn compile<F>(
        &self,
        tracer: &mut Tracer,
        main_source: F,
    ) -> Result<Document, TypstAsLibError>
    where
        F: Into<FileIdNewType>,
    {
        let Self { sources, .. } = self;
        let FileIdNewType(main_source) = main_source.into();
        let main_source = sources
            .get(&main_source)
            .ok_or_else(|| TypstAsLibError::MainSourceFileDoesNotExist(main_source))?;
        let world = TypstWorld {
            library: Default::default(),
            collection: self,
            main_source,
        };
        let doc = typst::compile(&world, tracer)?;
        Ok(doc)
    }
}

fn initialize_library<D>(collection: &TypstTemplateCollection, inputs: D) -> Library
where
    D: Into<Dict>,
{
    let inputs = inputs.into();
    let TypstTemplateCollection {
        inject_location, ..
    } = collection;
    if let Some(InjectLocation {
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
    }
}

struct TypstWorld<'a> {
    library: Prehashed<Library>,
    main_source: &'a Source,
    collection: &'a TypstTemplateCollection,
}

impl typst::World for TypstWorld<'_> {
    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    fn book(&self) -> &Prehashed<FontBook> {
        &self.collection.book
    }

    fn main(&self) -> Source {
        self.main_source.clone()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let TypstWorld {
            collection: TypstTemplateCollection { sources, .. },
            ..
        } = self;

        if let Some(source) = sources.get(&id).cloned() {
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
            collection: TypstTemplateCollection { files, .. },
            ..
        } = self;

        files
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.vpath().as_rooted_path().to_path_buf()))
    }

    fn font(&self, id: usize) -> Option<Font> {
        self.collection.fonts.get(id).cloned()
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

#[derive(Debug, Clone)]
struct InjectLocation {
    preinitialized_library: Library,
    module_name: String,
    value_name: String,
}

#[derive(Debug, Clone, Error)]
pub enum TypstAsLibError {
    #[error("Typst source error: {}", 0.to_string())]
    TypstSource(EcoVec<SourceDiagnostic>),
    #[error("Typst file error: {0}")]
    TypstFile(#[from] FileError),
    #[error("Source file does not exist in collection")]
    MainSourceFileDoesNotExist(FileId),
}

impl From<EcoVec<SourceDiagnostic>> for TypstAsLibError {
    fn from(value: EcoVec<SourceDiagnostic>) -> Self {
        TypstAsLibError::TypstSource(value)
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
