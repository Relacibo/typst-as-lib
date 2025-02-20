use std::borrow::Cow;
use std::ops::Deref;
use std::path::PathBuf;

use cached_file_resolver::IntoCachedFileResolver;
use chrono::{DateTime, Datelike, Duration, Utc};
use ecow::EcoVec;
use file_resolver::{
    FileResolver, FileSystemResolver, MainSourceFileResolver, StaticFileResolver,
    StaticSourceFileResolver,
};
use thiserror::Error;
use typst::diag::{FileError, FileResult, HintedString, SourceDiagnostic, Warned};
use typst::foundations::{Bytes, Datetime, Dict, Module, Scope, Value};
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Document, Library};
use util::not_found;

pub mod cached_file_resolver;
pub mod file_resolver;
pub(crate) mod util;

#[cfg(feature = "packages")]
pub mod package_resolver;

#[cfg(feature = "typst-kit-fonts")]
pub mod font_searcher_options;

// Inspired by https://github.com/tfachmann/typst-as-library/blob/main/src/lib.rs
pub struct TypstTemplateCollection {
    book: LazyHash<FontBook>,
    inject_location: Option<InjectLocation>,
    file_resolvers: Vec<Box<dyn FileResolver + Send + Sync + 'static>>,
    library: LazyHash<Library>,
    comemo_evict_max_age: Option<usize>,
    #[cfg(not(feature = "typst-kit-fonts"))]
    fonts: Vec<Font>,
    #[cfg(feature = "typst-kit-fonts")]
    fonts: Option<Vec<typst_kit::fonts::FontSlot>>,
}
impl Default for TypstTemplateCollection {
    fn default() -> Self {
        Self {
            book: LazyHash::new(FontBook::new()),
            inject_location: Default::default(),
            file_resolvers: Default::default(),
            library: Default::default(),
            comemo_evict_max_age: Some(0),
            #[cfg(not(feature = "typst-kit-fonts"))]
            fonts: Default::default(),
            #[cfg(feature = "typst-kit-fonts")]
            fonts: None,
        }
    }
}

impl TypstTemplateCollection {
    /// Initialize with fonts.
    ///
    /// Example:
    /// ```rust
    /// static TEMPLATE: &str = include_str!("./templates/template.typ");
    /// static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
    /// // ...
    /// let font = Font::new(Bytes::from(FONT), 0)
    ///     .expect("Could not parse font!");
    /// let template = TypstTemplateCollection::new()
    ///     .add_fonts([font])
    ///     .with_static_file_resolver([TEMPLATE], []);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    pub fn custom_inject_location(
        mut self,
        module_name: &'static str,
        value_name: &'static str,
    ) -> Self {
        self.custom_inject_location_mut(module_name, value_name);
        self
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    pub fn custom_inject_location_mut(
        &mut self,
        module_name: &'static str,
        value_name: &'static str,
    ) {
        self.inject_location = Some(InjectLocation {
            module_name,
            value_name,
        });
    }

    /// Add Fonts
    #[cfg(not(feature = "typst-kit-fonts"))]
    pub fn add_fonts<I, F>(mut self, fonts: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        self.add_fonts_mut(fonts);
        self
    }

    /// Add Fonts
    #[cfg(not(feature = "typst-kit-fonts"))]
    pub fn add_fonts_mut<I, F>(&mut self, fonts: I) -> &mut Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        let fonts = fonts.into_iter().map(Into::into).collect::<Vec<_>>();
        for f in fonts.iter() {
            self.book.push(f.info().clone())
        }
        self.fonts.extend(fonts);
        self
    }

    /// Use typst_kit::fonts::FontSearcher when looking up fonts
    /// ```rust
    /// // ...
    /// let font = Font::new(Bytes::from(FONT), 0)
    ///     .expect("Could not parse font!");
    ///
    /// let template = TypstTemplateCollection::new()
    ///     .search_fonts_with(Default::default())
    ///     .with_static_file_resolver([TEMPLATE], []);
    /// ```
    #[cfg(feature = "typst-kit-fonts")]
    pub fn search_fonts_with<I, P>(
        mut self,
        options: font_searcher_options::FontSearcherOptions<I, P>,
    ) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        self.with_font_searcher_mut(options);
        self
    }

    #[cfg(feature = "typst-kit-fonts")]
    pub fn with_font_searcher_mut<I, P>(
        &mut self,
        options: font_searcher_options::FontSearcherOptions<I, P>,
    ) -> &mut Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        let typst_kit::fonts::Fonts { book, fonts } = typst_kit::fonts::Fonts::searcher()
            .include_system_fonts(options.include_system_fonts)
            .search_with(options.include_dirs);

        self.book = LazyHash::new(book);
        self.fonts = Some(fonts);
        self
    }

    /// Add file resolver, that implements the `FileResolver`` trait to a vec of file resolvers.
    /// When a `FileId`` needs to be resolved by Typst, the vec will be iterated over until
    /// one file resolver returns a file.
    pub fn add_file_resolver<F>(mut self, file_resolver: F) -> Self
    where
        F: FileResolver + Send + Sync + 'static,
    {
        self.add_file_resolver_mut(file_resolver);
        self
    }

    /// Add file resolver, that implements the `FileResolver`` trait to a vec of file resolvers.
    /// When a `FileId`` needs to be resolved by Typst, the vec will be iterated over until
    /// one file resolver returns a file.
    pub fn add_file_resolver_mut<F>(&mut self, file_resolver: F)
    where
        F: FileResolver + Send + Sync + 'static,
    {
        self.file_resolvers.push(Box::new(file_resolver));
    }

    /// Adds the `StaticSourceFileResolver` to the file resolvers. It creates `HashMap`s for sources.
    ///
    /// `sources` The item of the IntoIterator can be of types:
    ///   - `&str/String`, creating a detached Source (Has vpath `/main.typ`)
    ///   - `(&str, &str/String)`, where &str is the absolute
    ///     virtual path of the Source file.
    ///   - `(typst::syntax::FileId, &str/String)`
    ///   - `typst::syntax::Source`
    ///
    /// (`&str/String` is always the template file content)
    pub fn with_static_source_file_resolver<IS, S>(mut self, sources: IS) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: Into<SourceNewType>,
    {
        self.with_static_source_file_resolver_mut(sources);
        self
    }

    /// Adds the `StaticSourceFileResolver` to the file resolvers. It creates `HashMap`s for sources.
    pub fn with_static_source_file_resolver_mut<IS, S>(&mut self, sources: IS)
    where
        IS: IntoIterator<Item = S>,
        S: Into<SourceNewType>,
    {
        self.add_file_resolver_mut(StaticSourceFileResolver::new(sources));
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for binaries.
    pub fn with_static_file_resolver<IB, F, B>(mut self, binaries: IB) -> Self
    where
        IB: IntoIterator<Item = (F, B)>,
        F: Into<FileIdNewType>,
        B: Into<BytesNewType>,
    {
        self.with_static_file_resolver_mut(binaries);
        self
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for binaries.
    pub fn with_static_file_resolver_mut<IB, F, B>(&mut self, binaries: IB)
    where
        IB: IntoIterator<Item = (F, B)>,
        F: Into<FileIdNewType>,
        B: Into<BytesNewType>,
    {
        self.add_file_resolver_mut(StaticFileResolver::new(binaries));
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
        self.add_file_resolver_mut(FileSystemResolver::new(root.into()).into_cached());
    }

    pub fn comemo_evict_max_age(&mut self, comemo_evict_max_age: Option<usize>) -> &mut Self {
        self.comemo_evict_max_age = comemo_evict_max_age;
        self
    }

    #[cfg(feature = "packages")]
    /// Adds `PackageResolver` to the file resolvers.
    /// When `package` is set in `FileId`, it will download the package from the typst package
    /// repository. It caches the results into `cache` (which is either in memory or cache folder (default)).
    /// Example
    /// ```rust
    ///     let template = TypstTemplateCollection::new(vec![font])
    ///         .with_package_file_resolver(None);
    /// ```
    pub fn with_package_file_resolver(mut self, ureq: Option<ureq::Agent>) -> Self {
        self.with_package_file_resolver_mut(ureq);
        self
    }

    #[cfg(feature = "packages")]
    pub fn with_package_file_resolver_mut(&mut self, ureq: Option<ureq::Agent>) {
        use package_resolver::PackageResolverBuilder;
        let mut builder = PackageResolverBuilder::new().with_file_system_cache();
        if let Some(ureq) = ureq {
            builder = builder.ureq_agent(ureq);
        }
        self.add_file_resolver_mut(builder.build().into_cached());
    }

    #[cfg(feature = "typst-kit-fonts")]
    pub fn get_fonts(&self) -> Option<&Vec<typst_kit::fonts::FontSlot>> {
        self.fonts.as_ref()
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
    /// let template_collection = TypstTemplateCollection::new().add_fonts([font])
    ///     .add_static_file_resolver([(TEMPLATE_ID, TEMPLATE)]);
    /// // Struct that implements Into<Dict>.
    /// let inputs = todo!();
    /// let tracer = Default::default();
    /// let doc = template_collection.compile_with_input(&mut tracer, TEMPLATE_ID, inputs)
    ///     .expect("Typst error!");
    /// ```
    pub fn compile_with_input<F, D, Doc>(
        &self,
        main_source_id: F,
        input: D,
    ) -> Warned<Result<Doc, TypstAsLibError>>
    where
        F: Into<FileIdNewType>,
        D: Into<Dict>,
        Doc: Document,
    {
        self.compile_helper(main_source_id, Some(input))
    }

    /// Just call `typst::compile()`
    pub fn compile<F, Doc>(&self, main_source_id: F) -> Warned<Result<Doc, TypstAsLibError>>
    where
        F: Into<FileIdNewType>,
        Doc: Document,
    {
        self.compile_helper::<_, Dict, _>(main_source_id, None)
    }

    fn compile_helper<F, D, Doc>(
        &self,
        main_source_id: F,
        inputs: Option<D>,
    ) -> Warned<Result<Doc, TypstAsLibError>>
    where
        F: Into<FileIdNewType>,
        D: Into<Dict>,
        Doc: Document,
    {
        let FileIdNewType(main_source_id) = main_source_id.into();
        let library = if let Some(inputs) = inputs {
            let lib = self.create_injected_library(inputs);
            match lib {
                Ok(lib) => Cow::Owned(lib),
                Err(err) => {
                    return Warned {
                        output: Err(err),
                        warnings: Default::default(),
                    };
                }
            }
        } else {
            Cow::Borrowed(&self.library)
        };
        let world = TypstWorld {
            collection: self,
            main_source_id,
            library,
            now: Utc::now(),
        };
        let Warned { output, warnings } = typst::compile(&world);

        if let Some(comemo_evict_max_age) = self.comemo_evict_max_age {
            comemo::evict(comemo_evict_max_age);
        }

        Warned {
            output: output.map_err(Into::into),
            warnings,
        }
    }

    fn create_injected_library<D>(&self, input: D) -> Result<LazyHash<Library>, TypstAsLibError>
    where
        D: Into<Dict>,
    {
        let Self {
            inject_location,
            library,
            ..
        } = self;
        let mut lib = library.deref().clone();
        inject_input_into_library(&mut lib, inject_location.as_ref(), input)?;
        Ok(LazyHash::new(lib))
    }

    fn resolve_file(&self, file_id: FileId) -> FileResult<Cow<Bytes>> {
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

    fn resolve_source(&self, file_id: FileId) -> FileResult<Cow<Source>> {
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

fn inject_input_into_library<'a, D>(
    library: &'a mut Library,
    inject_location: Option<&InjectLocation>,
    input: D,
) -> Result<&'a mut Library, TypstAsLibError>
where
    D: Into<Dict>,
{
    let (module_name, value_name) = if let Some(InjectLocation {
        module_name,
        value_name,
    }) = inject_location
    {
        (*module_name, *value_name)
    } else {
        ("sys", "inputs")
    };
    let global = library.global.scope_mut();
    let mut scope = Scope::new();
    scope.define(value_name, input.into());
    if let Some(value) = global.get_mut(module_name) {
        let value = value.write().map_err(TypstAsLibError::Unspecified)?;
        if let Value::Module(module) = value {
            *module.scope_mut() = scope;
        } else {
            let module = Module::new(module_name, scope);
            *value = Value::Module(module);
        }
    } else {
        let module = Module::new(module_name, scope);
        global.define(module_name, module);
    }
    Ok(library)
}

pub struct TypstTemplate {
    source_id: FileId,
    collection: TypstTemplateCollection,
}

impl TypstTemplate {
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
    /// let template = TypstTemplate::new(TEMPLATE).add_fonts([font]);
    /// ```
    pub fn new<S>(source_id: S) -> Self
    where
        S: Into<SourceNewType>,
    {
        let SourceNewType(source) = source_id.into();
        let source_id = source.id();
        let mut collection = TypstTemplateCollection::new();
        collection
            .file_resolvers
            .push(Box::new(MainSourceFileResolver::new(source)));
        Self {
            collection,
            source_id,
        }
    }

    pub fn comemo_evict_max_age(&mut self, comemo_evict_max_age: Option<usize>) -> &mut Self {
        self.collection.comemo_evict_max_age = comemo_evict_max_age;
        self
    }

    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    pub fn custom_inject_location(
        mut self,
        module_name: &'static str,
        value_name: &'static str,
    ) -> Self {
        self.collection
            .custom_inject_location_mut(module_name, value_name);
        self
    }

    /// Add Fonts
    #[cfg(not(feature = "typst-kit-fonts"))]
    pub fn add_fonts<I, F>(mut self, fonts: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: Into<Font>,
    {
        self.collection.add_fonts_mut(fonts);
        self
    }

    /// Use typst_kit::fonts::FontSearcher when looking up fonts
    /// ```rust
    /// // ...
    /// let font = Font::new(Bytes::from(FONT), 0)
    ///     .expect("Could not parse font!");
    ///
    /// let template = TypstTemplate::new(TEMPLATE)
    ///     .search_fonts_with(Default::default());
    /// ```
    #[cfg(feature = "typst-kit-fonts")]
    pub fn search_fonts_with<I, P>(
        mut self,
        options: font_searcher_options::FontSearcherOptions<I, P>,
    ) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        self.collection.with_font_searcher_mut(options);
        self
    }

    /// Add file resolver, that implements the `FileResolver`` trait to a vec of file resolvers.
    /// When a `FileId`` needs to be resolved by Typst, the vec will be iterated over until
    /// one file resolver returns a file.
    pub fn add_file_resolver<F>(mut self, file_resolver: F) -> Self
    where
        F: FileResolver + Send + Sync + 'static,
    {
        self.collection.add_file_resolver_mut(file_resolver);
        self
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for sources.
    ///
    /// `sources` The item of the IntoIterator can be of types:
    ///   - `&str/String`, creating a detached Source (Has vpath `/main.typ`)
    ///   - `(&str, &str/String)`, where &str is the absolute
    ///     virtual path of the Source file.
    ///   - `(typst::syntax::FileId, &str/String)`
    ///   - `typst::syntax::Source`
    ///
    /// (`&str/String` is always the template file content)
    pub fn with_static_source_file_resolver<IS, S>(mut self, sources: IS) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: Into<SourceNewType>,
    {
        self.collection
            .with_static_source_file_resolver_mut(sources);
        self
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for binaries.
    pub fn with_static_file_resolver<IB, F, B>(mut self, binaries: IB) -> Self
    where
        IB: IntoIterator<Item = (F, B)>,
        F: Into<FileIdNewType>,
        B: Into<BytesNewType>,
    {
        self.collection.with_static_file_resolver_mut(binaries);
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
    /// repository. It caches the results into `cache` (which is either in memory or cache folder (default)).
    /// Example
    /// ```rust
    ///     let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
    ///         .with_package_file_resolver(None);
    /// ```
    pub fn with_package_file_resolver(mut self, ureq: Option<ureq::Agent>) -> Self {
        self.collection.with_package_file_resolver_mut(ureq);
        self
    }

    /// Call `typst::compile()` with our template and a `Dict` as input, that will be availible
    /// in a typst script with `#import sys: inputs`.
    pub fn compile_with_input<D, Doc>(&self, inputs: D) -> Warned<Result<Doc, TypstAsLibError>>
    where
        D: Into<Dict>,
        Doc: Document,
    {
        let Self {
            source_id,
            collection,
            ..
        } = self;
        collection.compile_with_input(*source_id, inputs)
    }

    /// Just call `typst::compile()`
    pub fn compile<Doc>(&self) -> Warned<Result<Doc, TypstAsLibError>>
    where
        Doc: Document,
    {
        let Self {
            source_id,
            collection,
            ..
        } = self;
        collection.compile(*source_id)
    }
}

struct TypstWorld<'a> {
    main_source_id: FileId,
    collection: &'a TypstTemplateCollection,
    library: Cow<'a, LazyHash<Library>>,
    now: DateTime<Utc>,
}

impl typst::World for TypstWorld<'_> {
    fn library(&self) -> &LazyHash<Library> {
        self.library.as_ref()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.collection.book
    }

    fn main(&self) -> FileId {
        self.main_source_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.collection.resolve_source(id).map(|s| s.into_owned())
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.collection.resolve_file(id).map(|b| b.into_owned())
    }

    fn font(&self, id: usize) -> Option<Font> {
        #[cfg(not(feature = "typst-kit-fonts"))]
        let res = self.collection.fonts.get(id).cloned();

        #[cfg(feature = "typst-kit-fonts")]
        let res = { self.collection.fonts.as_ref()?[id].get() };
        res
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        let mut now = self.now;
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
    module_name: &'static str,
    value_name: &'static str,
}

#[derive(Debug, Clone, Error)]
pub enum TypstAsLibError {
    #[error("Typst source error: {}", 0.to_string())]
    TypstSource(EcoVec<SourceDiagnostic>),
    #[error("Typst file error: {}", 0.to_string())]
    TypstFile(#[from] FileError),
    #[error("Source file does not exist in collection: {0:?}")]
    MainSourceFileDoesNotExist(FileId),
    #[error("Typst hinted String: {}", 0.to_string())]
    HintedString(HintedString),
    #[error("Unspecified: {0}!")]
    Unspecified(ecow::EcoString),
}

impl From<HintedString> for TypstAsLibError {
    fn from(value: HintedString) -> Self {
        TypstAsLibError::HintedString(value)
    }
}

impl From<EcoVec<SourceDiagnostic>> for TypstAsLibError {
    fn from(value: EcoVec<SourceDiagnostic>) -> Self {
        TypstAsLibError::TypstSource(value)
    }
}

#[derive(Clone, Debug, Hash)]
pub struct FileIdNewType(pub FileId);

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
pub struct SourceNewType(pub Source);

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

#[derive(Clone, Debug, Hash)]
pub struct BytesNewType(pub Bytes);

impl From<Bytes> for BytesNewType {
    fn from(bytes: Bytes) -> Self {
        BytesNewType(bytes)
    }
}

impl From<&[u8]> for BytesNewType {
    fn from(bytes: &[u8]) -> Self {
        BytesNewType(Bytes::new(bytes.to_vec()))
    }
}

impl From<TypstTemplate> for TypstTemplateCollection {
    fn from(value: TypstTemplate) -> Self {
        value.collection
    }
}
