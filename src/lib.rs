use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{Datelike, Duration, Local};
use comemo::Prehashed;
use ecow::EcoVec;
use file_resolver::{FileResolver, FileSystemResolver, MainSourceFileResolver, StaticFileResolver};
use thiserror::Error;
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime, Dict, Module, Scope};
use typst::model::Document;
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::Library;
use util::not_found;

pub mod file_resolver;
pub(crate) mod util;

#[cfg(feature = "packages")]
pub mod package_resolver;
#[cfg(feature = "packages")]
use package_resolver::PackageResolver;

// Inspired by https://github.com/tfachmann/typst-as-library/blob/main/src/lib.rs

pub struct TypstTemplateCollection<'a> {
    book: Prehashed<FontBook>,
    fonts: Vec<Font>,
    inject_location: Option<InjectLocation>,
    file_resolvers: Vec<Box<dyn FileResolver + 'a>>,
}

impl<'a> TypstTemplateCollection<'a> {
    /// Initialize with fonts.
    ///
    /// Example:
    /// ```rust
    /// static TEMPLATE: &str = include_str!("./templates/template.typ");
    /// static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
    /// // ...
    /// let font = Font::new(Bytes::from(FONT), 0)
    ///     .expect("Could not parse font!");
    /// let template = TypstTemplate::new(vec![font])
    ///     .with_static_file_resolver([TEMPLATE], []);
    /// ```
    pub fn new<V>(fonts: V) -> Self
    where
        V: Into<Vec<Font>>,
    {
        let fonts = fonts.into();
        Self {
            book: Prehashed::new(FontBook::from_fonts(&fonts)),
            fonts,
            inject_location: Default::default(),
            file_resolvers: Default::default(),
        }
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    /// Also preinitializes the library for better performance,
    /// if the template will be reused.
    pub fn custom_inject_location<S>(mut self, module_name: S, value_name: S) -> Self
    where
        S: Into<String>,
    {
        self.custom_inject_location_mut(module_name, value_name);
        self
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    /// Also preinitializes the library for better performance,
    /// if the template will be reused.
    pub fn custom_inject_location_mut<S>(&mut self, module_name: S, value_name: S)
    where
        S: Into<String>,
    {
        self.inject_location = Some(InjectLocation {
            preinitialized_library: Default::default(),
            module_name: module_name.into(),
            value_name: value_name.into(),
        });
    }

    /// Add Fonts
    pub fn add_fonts<I, F>(mut self, fonts: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        self.add_fonts_mut(fonts);
        self
    }

    /// Add Fonts
    pub fn add_fonts_mut<I, F>(&mut self, fonts: I) -> &mut Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        let fonts = fonts.into_iter().map(Into::into);
        self.fonts.extend(fonts);
        self
    }

    /// Add file resolver, that implements the `FileResolver`` trait to a vec of file resolvers.
    /// When a `FileId`` needs to be resolved by Typst, the vec will be iterated over until
    /// one file resolver returns a file.
    pub fn add_file_resolver<F>(mut self, file_resolver: F) -> Self
    where
        F: FileResolver + 'a,
    {
        self.add_file_resolver_mut(file_resolver);
        self
    }

    /// Add file resolver, that implements the `FileResolver`` trait to a vec of file resolvers.
    /// When a `FileId`` needs to be resolved by Typst, the vec will be iterated over until
    /// one file resolver returns a file.
    pub fn add_file_resolver_mut<F>(&mut self, file_resolver: F)
    where
        F: FileResolver + 'a,
    {
        self.file_resolvers.push(Box::new(file_resolver));
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for each sources
    /// and binaries.
    ///
    /// `sources` The item of the IntoIterator can be of types:
    ///   - `&str/String`, creating a detached Source (Has vpath `/main.typ`)
    ///   - `(&str, &str/String)`, where &str is the absolute
    ///     virtual path of the Source file.
    ///   - `(typst::syntax::FileId, &str/String)`
    ///   - `typst::syntax::Source`
    ///
    pub fn with_static_file_resolver<IS, S, IB, F, B>(mut self, sources: IS, binaries: IB) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: IntoSource,
        IB: IntoIterator<Item = (F, B)>,
        F: IntoFileId,
        B: IntoBytes,
    {
        self.with_static_file_resolver_mut(sources, binaries);
        self
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for each sources
    /// and binaries.
    pub fn with_static_file_resolver_mut<IS, S, IB, F, B>(&mut self, sources: IS, binaries: IB)
    where
        IS: IntoIterator<Item = S>,
        S: IntoSource,
        IB: IntoIterator<Item = (F, B)>,
        F: IntoFileId,
        B: IntoBytes,
    {
        self.add_file_resolver_mut(StaticFileResolver::new(sources, binaries));
    }

    /// Adds `FileSystemResolver` to the file resolvers, a resolver that can resolve
    /// local files (when `package` is not set in `FileId`).
    pub fn with_file_system_resolver<P>(mut self, root: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.with_file_system_resolver_mut(root);
        self
    }

    /// Adds `FileSystemResolver` to the file resolvers, a resolver that can resolve
    /// local files (when `package` is not set in `FileId`).
    pub fn with_file_system_resolver_mut<P>(&mut self, root: P)
    where
        P: Into<PathBuf>,
    {
        self.add_file_resolver_mut(FileSystemResolver::new(root.into()));
    }

    #[cfg(feature = "packages")]
    /// Adds `PackageResolver` to the file resolvers.
    /// When `package` is set in `FileId`, it will download the package from the typst package
    /// repository. It caches the results into `cache`.
    pub fn with_package_file_resolver(
        mut self,
        cache: Rc<RefCell<HashMap<FileId, Vec<u8>>>>,
        ureq: Option<ureq::Agent>,
    ) -> Self {
        self.with_package_file_resolver_mut(cache, ureq);
        self
    }

    #[cfg(feature = "packages")]
    /// Adds `PackageResolver` to the file resolvers.
    /// When `package` is set in `FileId`, it will download the package from the typst package
    /// repository. It caches the results into `cache`.
    pub fn with_package_file_resolver_mut(
        &mut self,
        cache: Rc<RefCell<HashMap<FileId, Vec<u8>>>>,
        ureq: Option<ureq::Agent>,
    ) {
        self.add_file_resolver_mut(PackageResolver::new(cache, ureq));
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
    ///     .add_static_file_resolver([(TEMPLATE_ID, TEMPLATE)]);
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
        F: IntoFileId,
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
        F: IntoFileId,
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
        F: IntoFileId,
    {
        let main_source_id = main_source_id.into_file_id();
        let main_source = self.resolve_source(main_source_id)?;
        let world = TypstWorld {
            library: Prehashed::new(library),
            collection: self,
            main_source,
        };
        let doc = typst::compile(&world, tracer)?;
        Ok(doc)
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

    fn resolve_file(&self, file_id: FileId) -> FileResult<Bytes> {
        let TypstTemplateCollection { file_resolvers, .. } = self;
        let mut last_error = not_found(file_id);
        for file_resolver in file_resolvers {
            match file_resolver.resolve_binary(file_id) {
                Ok(source) => return Ok(source),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    fn resolve_source(&self, file_id: FileId) -> FileResult<Source> {
        let TypstTemplateCollection { file_resolvers, .. } = self;
        let mut last_error = not_found(file_id);
        for file_resolver in file_resolvers {
            match file_resolver.resolve_source(file_id) {
                Ok(source) => return Ok(source),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }
}

pub struct TypstTemplate<'a> {
    source_id: FileId,
    collection: TypstTemplateCollection<'a>,
}

impl<'a> TypstTemplate<'a> {
    /// Initialize with fonts and a source file.
    ///
    /// `source` can be of types:
    ///   - `&str/String`, creating a detached Source (Has vpath `/main.typ`)
    ///   - `(&str, &str/String)`, where &str is the absolute
    ///     virtual path of the Source file.
    ///   - `(typst::syntax::FileId, &str/String)`
    ///   - `typst::syntax::Source`
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
        S: IntoSource,
    {
        let source = source.into_source();
        let source_id = source.id();
        let mut collection = TypstTemplateCollection::new(fonts);
        collection
            .file_resolvers
            .push(Box::new(MainSourceFileResolver::new(source)));
        Self {
            collection,
            source_id,
        }
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    /// Also preinitializes the library for better performance,
    /// if the template will be reused.
    pub fn custom_inject_location<S>(mut self, module_name: S, value_name: S) -> Self
    where
        S: Into<String>,
    {
        self.collection
            .custom_inject_location_mut(module_name, value_name);
        self
    }

    /// Add Fonts
    pub fn add_fonts<I, F>(mut self, fonts: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        self.collection.add_fonts_mut(fonts);
        self
    }

    /// Add file resolver, that implements the `FileResolver`` trait to a vec of file resolvers.
    /// When a `FileId`` needs to be resolved by Typst, the vec will be iterated over until
    /// one file resolver returns a file.
    pub fn add_file_resolver<F>(mut self, file_resolver: F) -> Self
    where
        F: FileResolver + 'a,
    {
        self.collection.add_file_resolver_mut(file_resolver);
        self
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for each sources
    /// and binaries.
    ///
    /// `sources` The item of the IntoIterator can be of types:
    ///   - `&str/String`, creating a detached Source (Has vpath `/main.typ`)
    ///   - `(&str, &str/String)`, where &str is the absolute
    ///     virtual path of the Source file.
    ///   - `(typst::syntax::FileId, &str/String)`
    ///   - `typst::syntax::Source`
    ///
    /// (`&str/String` is always the template file content)
    pub fn with_static_file_resolver<IS, S, IB, B, F>(mut self, sources: IS, binaries: IB) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: IntoSource,
        IB: IntoIterator<Item = (F, B)>,
        F: IntoFileId,
        B: IntoBytes,
    {
        self.collection
            .with_static_file_resolver_mut(sources, binaries);
        self
    }

    /// Adds `FileSystemFileResolver` to the file resolvers, a resolver that can resolve
    /// local files (when `package` is not set in `FileId`).
    pub fn with_file_system_resolver<P>(mut self, root: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.collection.with_file_system_resolver_mut(root);
        self
    }

    #[cfg(feature = "packages")]
    /// Adds `PackageResolver` to the file resolvers.
    /// When `package` is set in `FileId`, it will download the package from the typst package
    /// repository. It caches the results into `cache`.
    pub fn with_package_file_resolver(
        mut self,
        cache: Rc<RefCell<HashMap<FileId, Vec<u8>>>>,
        ureq: Option<ureq::Agent>,
    ) -> Self {
        self.collection.with_package_file_resolver_mut(cache, ureq);
        self
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
    main_source: Source,
    collection: &'a TypstTemplateCollection<'a>,
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
        self.collection.resolve_source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.collection.resolve_file(id)
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

pub trait IntoFileId {
    fn into_file_id(self) -> FileId;
}

impl IntoFileId for FileId {
    fn into_file_id(self) -> FileId {
        self
    }
}

impl IntoFileId for &str {
    fn into_file_id(self) -> FileId {
        FileId::new(None, VirtualPath::new(self))
    }
}

impl IntoFileId for (PackageSpec, &str) {
    fn into_file_id(self) -> FileId {
        let (p, id) = self;
        FileId::new(Some(p), VirtualPath::new(id))
    }
}

pub trait IntoSource {
    fn into_source(self) -> Source;
}

impl IntoSource for Source {
    fn into_source(self) -> Source {
        self
    }
}

impl IntoSource for (&str, String) {
    fn into_source(self) -> Source {
        let (path, source) = self;
        let id = FileId::new(None, VirtualPath::new(path));
        let source = Source::new(id, source);
        source
    }
}

impl IntoSource for (&str, &str) {
    fn into_source(self) -> Source {
        let (path, source) = self;
        (path, source.to_owned()).into_source()
    }
}

impl IntoSource for (FileId, String) {
    fn into_source(self) -> Source {
        let (id, source) = self;
        let source = Source::new(id, source);
        source
    }
}

impl IntoSource for (FileId, &str) {
    fn into_source(self) -> Source {
        let (id, source) = self;
        (id, source.to_owned()).into_source()
    }
}

impl IntoSource for String {
    fn into_source(self) -> Source {
        let source = Source::detached(self);
        source
    }
}

impl IntoSource for &str {
    fn into_source(self) -> Source {
        self.to_owned().into_source()
    }
}

trait IntoBytes {
    fn into_bytes(self) -> Bytes;
}

impl IntoBytes for Bytes {
    fn into_bytes(self) -> Bytes {
        self
    }
}

impl IntoBytes for Vec<u8> {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self)
    }
}

impl IntoBytes for &[u8] {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self)
    }
}

impl<'a> From<TypstTemplate<'a>> for TypstTemplateCollection<'a> {
    fn from(value: TypstTemplate<'a>) -> Self {
        value.collection
    }
}
