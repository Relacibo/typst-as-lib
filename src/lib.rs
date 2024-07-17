use std::borrow::Cow;
use std::collections::HashMap;
use std::iter;

use chrono::{Datelike, Duration, Local};
use comemo::Prehashed;
use ecow::EcoVec;
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

pub struct TypstTemplateCollection {
    book: Prehashed<FontBook>,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
    fonts: Vec<Font>,
    inject_location: Option<InjectLocation>,
    file_resolver: Option<Box<dyn Fn(FileId) -> FileResult<Bytes>>>,
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
            file_resolver: Default::default(),
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
    /// static SOURCE: &str = include_str!("./templates/source.typ");
    /// // ...
    /// let source = ("/source.typ", SOURCE);
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

    /// Set optional file resolver
    /// Read source files, packages and binaries dynamically during `typst::compile`.
    /// Example:
    /// ```rust
    /// static TEMPLATE_FILE: &str = include_str!("./templates/resolve_files.typ");

    /// static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

    /// static OUTPUT: &str = "./examples/output.pdf";
    /// // in main:
    /// let template = TypstTemplate::new(vec![font], TEMPLATE_FILE).file_resolver(resolve_files);
    /// let mut tracer = Default::default();
    /// let doc = template
    ///     .compile(&mut tracer)
    ///     .expect("typst::compile() returned an error!");
    /// // Create pdf
    /// let pdf = typst_pdf::pdf(&doc, Smart::Auto, None);
    /// fs::write(OUTPUT, pdf).expect("Could not write pdf.");
    /// ```
    /// For the code of the `resolve_files` function see
    /// [example](https://github.com/Relacibo/typst-as-lib/blob/1a8fd0ffcee9fad81c5df894c331a2af7c169cff/examples/resolve_files.rs#L45).
    pub fn file_resolver<F>(self, file_resolver: F) -> Self
    where
        F: Fn(FileId) -> FileResult<Bytes> + 'static,
    {
        Self {
            file_resolver: Some(Box::new(file_resolver)),
            ..self
        }
    }

    /// Call `typst::compile()` with our template and a `Dict` as input, that will be availible
    /// in a typst script with `#import sys: inputs`.
    ///
    /// Example:
    ///
    /// ```rust
    /// static TEMPLATE: &str = include_str!("./templates/template.typ");
    /// static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
    /// static TEMPLATE_ID: &str = "/template.typ";
    /// // ...
    /// let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");
    /// let template_collection = TypstTemplateCollection::new(vec![font])
    ///     .add_sources([(TEMPLATE_ID, TEMPLATE)]);
    /// // Struct that implements Into<Dict>.
    /// let inputs = todo!();
    /// let tracer = Default::default();
    /// let doc = template_collection.compile_with_inputs(&mut tracer, TEMPLATE_ID, inputs)
    ///     .expect("Typst error!");
    /// ```
    pub fn compile_with_input<F, D>(
        &self,
        tracer: &mut Tracer,
        main_source_id: F,
        inputs: D,
    ) -> Result<Document, TypstAsLibError>
    where
        F: Into<FileIdNewType>,
        D: Into<Dict>,
    {
        let library = self.initialize_library(inputs)?;
        self.compile_with_library(tracer, library, main_source_id)
    }

    /// Just call `typst::compile()`
    pub fn compile<F>(
        &self,
        tracer: &mut Tracer,
        main_source_id: F,
    ) -> Result<Document, TypstAsLibError>
    where
        F: Into<FileIdNewType>,
    {
        self.compile_with_library(tracer, Default::default(), main_source_id)
    }

    fn compile_with_library<F>(
        &self,
        tracer: &mut Tracer,
        library: Library,
        main_source_id: F,
    ) -> Result<Document, TypstAsLibError>
    where
        F: Into<FileIdNewType>,
    {
        let FileIdNewType(main_source_id) = main_source_id.into();
        let Self { sources, .. } = self;
        let main_source = sources.get(&main_source_id);
        let main_source = if let Some(main_source) = main_source {
            Cow::Borrowed(main_source)
        } else {
            let source = self.resolve_with_file_resolver(main_source_id)?;
            Cow::Owned(source)
        };
        let world = TypstWorld {
            library: Prehashed::new(library),
            collection: self,
            main_source,
        };
        let doc = typst::compile(&world, tracer)?;
        Ok(doc)
    }

    fn resolve_with_file_resolver(&self, id: FileId) -> Result<Source, FileError> {
        let TypstTemplateCollection { file_resolver, .. } = self;
        let Some(file_resolver) = file_resolver else {
            return Err(FileError::NotFound(
                id.vpath().as_rooted_path().to_path_buf(),
            ));
        };
        // https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L78
        let file = file_resolver(id)?;
        let contents = std::str::from_utf8(&file).map_err(|_| FileError::InvalidUtf8)?;
        let contents = contents.trim_start_matches('\u{feff}');
        Ok(Source::new(id, contents.into()))
    }

    fn initialize_library<D>(&self, inputs: D) -> Result<Library, TypstAsLibError>
    where
        D: Into<Dict>,
    {
        let inputs = inputs.into();
        let TypstTemplateCollection {
            inject_location, ..
        } = self;
        let lib = if let Some(InjectLocation {
            preinitialized_library,
            module_name,
            value_name,
        }) = inject_location
        {
            let mut lib = preinitialized_library.clone();
            let global = lib.global.scope_mut();
            if global.get(module_name).is_some() {
                return Err(TypstAsLibError::InjectLocationIsNotEmpty);
            }
            let mut scope = Scope::new();
            scope.define(value_name, inputs);
            let module = Module::new(module_name, scope);
            global.define_module(module);
            lib
        } else {
            Library::builder().with_inputs(inputs).build()
        };
        Ok(lib)
    }
}

pub struct TypstTemplate {
    source_id: FileId,
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
        let SourceNewType(source) = source.into();
        let source_id = source.id();
        let collection = TypstTemplateCollection::new(fonts).add_sources(iter::once(source));
        Self {
            collection,
            source_id,
        }
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
    pub fn source<S>(self, source: S) -> Self
    where
        S: Into<SourceNewType>,
    {
        let SourceNewType(source) = source.into();
        let source_id = source.id();
        let collection = self.collection.add_sources(iter::once(source));
        Self {
            source_id,
            collection,
            ..self
        }
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    /// Also preinitializes the library for better performance,
    /// if the template will be reused.
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

    /// Set optional file resolver
    /// Read source files, packages and binaries dynamically during `typst::compile`.
    /// Example:
    /// ```rust
    /// static TEMPLATE_FILE: &str = include_str!("./templates/resolve_files.typ");

    /// static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

    /// static OUTPUT: &str = "./examples/output.pdf";
    /// // in main:
    /// let template = TypstTemplate::new(vec![font], TEMPLATE_FILE).file_resolver(resolve_files);
    /// let mut tracer = Default::default();
    /// let doc = template
    ///     .compile(&mut tracer)
    ///     .expect("typst::compile() returned an error!");
    /// // Create pdf
    /// let pdf = typst_pdf::pdf(&doc, Smart::Auto, None);
    /// fs::write(OUTPUT, pdf).expect("Could not write pdf.");
    /// ```
    /// For the code of the `resolve_files` function see
    /// [example](https://github.com/Relacibo/typst-as-lib/blob/1a8fd0ffcee9fad81c5df894c331a2af7c169cff/examples/resolve_files.rs#L45).
    pub fn file_resolver<F>(self, f: F) -> Self
    where
        F: Fn(FileId) -> FileResult<Bytes> + 'static,
    {
        Self {
            collection: self.collection.file_resolver(f),
            ..self
        }
    }

    /// Call `typst::compile()` with our template and a `Dict` as input, that will be availible
    /// in a typst script with `#import sys: inputs`.
    pub fn compile_with_input<D>(
        &self,
        tracer: &mut Tracer,
        inputs: D,
    ) -> Result<Document, TypstAsLibError>
    where
        D: Into<Dict>,
    {
        let Self {
            source_id,
            collection,
            ..
        } = self;
        collection.compile_with_input(tracer, *source_id, inputs)
    }

    /// Just call `typst::compile()`
    pub fn compile(&self, tracer: &mut Tracer) -> Result<Document, TypstAsLibError> {
        let Self {
            source_id,
            collection,
            ..
        } = self;
        collection.compile(tracer, *source_id)
    }
}

struct TypstWorld<'a> {
    library: Prehashed<Library>,
    main_source: Cow<'a, Source>,
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
        self.main_source.as_ref().clone()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let Self {
            collection,
            main_source,
            ..
        } = self;
        if id == main_source.id() {
            return Ok(main_source.as_ref().clone());
        }
        let TypstTemplateCollection { sources, .. } = collection;

        if let Some(source) = sources.get(&id).cloned() {
            return Ok(source);
        }

        self.collection.resolve_with_file_resolver(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let TypstWorld {
            collection:
                TypstTemplateCollection {
                    files,
                    file_resolver,
                    ..
                },
            ..
        } = self;

        if let Some(bytes) = files.get(&id).cloned() {
            return Ok(bytes);
        }

        if let Some(file_resolver) = file_resolver {
            return file_resolver(id);
        }

        Err(FileError::NotFound(
            id.vpath().as_rooted_path().to_path_buf(),
        ))
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
    #[error("Typst file error: {}", 0.to_string())]
    TypstFile(#[from] FileError),
    #[error("Source file does not exist in collection: {0:?}")]
    MainSourceFileDoesNotExist(FileId),
    #[error("Library could not be initialized. Inject location is not empty.")]
    InjectLocationIsNotEmpty,
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

impl From<TypstTemplate> for TypstTemplateCollection {
    fn from(value: TypstTemplate) -> Self {
        value.collection
    }
}
