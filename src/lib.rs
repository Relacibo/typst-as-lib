// Inspired by https://github.com/tfachmann/typst-as-library/blob/main/src/lib.rs
use std::borrow::Cow;
use std::ops::Deref;
use std::path::PathBuf;

use cached_file_resolver::IntoCachedFileResolver;
use chrono::{DateTime, Datelike, Duration, Utc};
use conversions::{IntoBytes, IntoFileId, IntoFonts, IntoSource};
use ecow::EcoVec;
use file_resolver::{
    FileResolver, FileSystemResolver, MainSourceFileResolver, StaticFileResolver,
    StaticSourceFileResolver,
};
use thiserror::Error;
use typst::diag::{FileError, FileResult, HintedString, SourceDiagnostic, Warned};
use typst::foundations::{Bytes, Datetime, Dict, Module, Scope, Value};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Document, Library};
use util::not_found;

pub mod cached_file_resolver;
pub mod conversions;
pub mod file_resolver;
pub(crate) mod util;

#[cfg(all(feature = "packages", any(feature = "ureq", feature = "reqwest")))]
pub mod package_resolver;

#[cfg(feature = "typst-kit-fonts")]
pub mod typst_kit_options;

pub struct TypstEngine<T = TypstTemplateCollection> {
    template: T,
    book: LazyHash<FontBook>,
    inject_location: Option<InjectLocation>,
    file_resolvers: Vec<Box<dyn FileResolver + Send + Sync + 'static>>,
    library: LazyHash<Library>,
    comemo_evict_max_age: Option<usize>,
    fonts: Vec<FontEnum>,
}

#[derive(Debug, Clone, Copy)]
pub struct TypstTemplateCollection;

#[derive(Debug, Clone, Copy)]
pub struct TypstTemplateMainFile {
    source_id: FileId,
}

impl<T> TypstEngine<T> {
    fn do_compile<Doc>(
        &self,
        main_source_id: FileId,
        inputs: Option<Dict>,
    ) -> Warned<Result<Doc, TypstAsLibError>>
    where
        Doc: Document,
    {
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
            main_source_id,
            library,
            now: Utc::now(),
            file_resolvers: &self.file_resolvers,
            book: &self.book,
            fonts: &self.fonts,
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
        let (module_name, value_name) = if let Some(InjectLocation {
            module_name,
            value_name,
        }) = inject_location
        {
            (*module_name, *value_name)
        } else {
            ("sys", "inputs")
        };
        {
            let global = lib.global.scope_mut();
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
        }
        Ok(LazyHash::new(lib))
    }
}

impl TypstEngine<TypstTemplateCollection> {
    pub fn builder() -> TypstTemplateEngineBuilder {
        TypstTemplateEngineBuilder::default()
    }
}

impl TypstEngine<TypstTemplateCollection> {
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
    /// let template_collection = TypstEngine::builder().fonts([FONT])
    ///     .with_static_file_resolver([(TEMPLATE_ID, TEMPLATE)]).build();
    /// // Struct that implements Into<Dict>.
    /// let inputs = todo!();
    /// let tracer = Default::default();
    /// let doc = template_collection.compile_with_input(&mut tracer, TEMPLATE_ID, inputs)
    ///     .expect("Typst error!");
    /// ```
    pub fn compile_with_input<F, D, Doc>(
        &self,
        main_source_id: F,
        inputs: D,
    ) -> Warned<Result<Doc, TypstAsLibError>>
    where
        F: IntoFileId,
        D: Into<Dict>,
        Doc: Document,
    {
        self.do_compile(main_source_id.into_file_id(), Some(inputs.into()))
    }

    /// Just call `typst::compile()`. Same as Self::compile_with_input but without the input
    pub fn compile<F, Doc>(&self, main_source_id: F) -> Warned<Result<Doc, TypstAsLibError>>
    where
        F: IntoFileId,
        Doc: Document,
    {
        self.do_compile(main_source_id.into_file_id(), None)
    }
}

impl TypstEngine<TypstTemplateMainFile> {
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
    /// let template_collection = TypstEngine::builder()
    ///     .main_file(TEMPLATE).fonts([FONT]).build();
    /// // Struct that implements Into<Dict>.
    /// let inputs = todo!();
    /// let tracer = Default::default();
    /// let doc = template_collection.compile_with_input(&mut tracer, TEMPLATE_ID, inputs)
    ///     .expect("Typst error!");
    /// ```
    pub fn compile_with_input<D, Doc>(&self, inputs: D) -> Warned<Result<Doc, TypstAsLibError>>
    where
        D: Into<Dict>,
        Doc: Document,
    {
        let TypstTemplateMainFile { source_id } = self.template;
        self.do_compile(source_id, Some(inputs.into()))
    }

    /// Just call `typst::compile()`. Same as Self::compile_with_input but without the input
    pub fn compile<Doc>(&self) -> Warned<Result<Doc, TypstAsLibError>>
    where
        Doc: Document,
    {
        let TypstTemplateMainFile { source_id } = self.template;
        self.do_compile(source_id, None)
    }
}

pub struct TypstTemplateEngineBuilder<T = TypstTemplateCollection> {
    template: T,
    inject_location: Option<InjectLocation>,
    file_resolvers: Vec<Box<dyn FileResolver + Send + Sync + 'static>>,
    comemo_evict_max_age: Option<usize>,
    fonts: Option<Vec<Font>>,
    #[cfg(feature = "typst-kit-fonts")]
    typst_kit_font_options: Option<typst_kit_options::TypstKitFontOptions>,
}

impl Default for TypstTemplateEngineBuilder {
    fn default() -> Self {
        Self {
            template: TypstTemplateCollection,
            inject_location: Default::default(),
            file_resolvers: Default::default(),
            comemo_evict_max_age: Some(0),
            fonts: Default::default(),
            #[cfg(feature = "typst-kit-fonts")]
            typst_kit_font_options: None,
        }
    }
}

impl TypstTemplateEngineBuilder<TypstTemplateCollection> {
    /// Declare a main_file that is used for each compilation as a starting point. This is optional.
    pub fn main_file<S: IntoSource>(
        self,
        source: S,
    ) -> TypstTemplateEngineBuilder<TypstTemplateMainFile> {
        let source = source.into_source();
        let source_id = source.id();
        let template = TypstTemplateMainFile { source_id };
        let TypstTemplateEngineBuilder {
            inject_location,
            mut file_resolvers,
            comemo_evict_max_age,
            fonts,
            #[cfg(feature = "typst-kit-fonts")]
            typst_kit_font_options,
            ..
        } = self;
        file_resolvers.push(Box::new(MainSourceFileResolver::new(source)));
        TypstTemplateEngineBuilder {
            template,
            inject_location,
            file_resolvers,
            comemo_evict_max_age,
            fonts,
            #[cfg(feature = "typst-kit-fonts")]
            typst_kit_font_options,
        }
    }
}

impl<T> TypstTemplateEngineBuilder<T> {
    /// Use other typst location for injected inputs
    /// (instead of`#import sys: inputs`, where `sys` is the `module_name`
    /// and `inputs` is the `value_name`).
    pub fn custom_inject_location(
        mut self,
        module_name: &'static str,
        value_name: &'static str,
    ) -> Self {
        self.inject_location = Some(InjectLocation {
            module_name,
            value_name,
        });
        self
    }

    /// Fonts
    /// Accepts IntoIterator Items:
    ///   - &[u8]
    ///   - Vec<u8>
    ///   - Bytes
    ///   - Font
    pub fn fonts<I, F>(mut self, fonts: I) -> Self
    where
        I: IntoIterator<Item = F>,
        F: IntoFonts,
    {
        let fonts = fonts
            .into_iter()
            .flat_map(IntoFonts::into_fonts)
            .collect::<Vec<_>>();
        self.fonts = Some(fonts);
        self
    }

    /// Use typst_kit::fonts::FontSearcher when looking up fonts
    /// ```rust
    /// // ...
    ///
    /// let template = TypstEngine::builder()
    ///     .search_fonts_with(Default::default())
    ///     .with_static_file_resolver([TEMPLATE], [])
    ///     .build();
    /// ```
    #[cfg(feature = "typst-kit-fonts")]
    pub fn search_fonts_with(mut self, options: typst_kit_options::TypstKitFontOptions) -> Self {
        self.typst_kit_font_options = Some(options);
        self
    }

    /// Add file resolver, that implements the `FileResolver`` trait to a vec of file resolvers.
    /// When a `FileId`` needs to be resolved by Typst, the vec will be iterated over until
    /// one file resolver returns a file.
    pub fn add_file_resolver<F>(mut self, file_resolver: F) -> Self
    where
        F: FileResolver + Send + Sync + 'static,
    {
        self.file_resolvers.push(Box::new(file_resolver));
        self
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
    pub fn with_static_source_file_resolver<IS, S>(self, sources: IS) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: IntoSource,
    {
        self.add_file_resolver(StaticSourceFileResolver::new(sources))
    }

    /// Adds the `StaticFileResolver` to the file resolvers. It creates `HashMap`s for binaries.
    pub fn with_static_file_resolver<IB, F, B>(self, binaries: IB) -> Self
    where
        IB: IntoIterator<Item = (F, B)>,
        F: IntoFileId,
        B: IntoBytes,
    {
        self.add_file_resolver(StaticFileResolver::new(binaries))
    }

    /// Adds `FileSystemResolver` to the file resolvers, a resolver that can resolve
    /// local files (when `package` is not set in `FileId`).
    pub fn with_file_system_resolver<P>(self, root: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.add_file_resolver(FileSystemResolver::new(root.into()).into_cached())
    }

    pub fn comemo_evict_max_age(&mut self, comemo_evict_max_age: Option<usize>) -> &mut Self {
        self.comemo_evict_max_age = comemo_evict_max_age;
        self
    }

    #[cfg(all(feature = "packages", any(feature = "ureq", feature = "reqwest")))]
    /// Adds `PackageResolver` to the file resolvers.
    /// When `package` is set in `FileId`, it will download the package from the typst package
    /// repository. It caches the results into `cache` (which is either in memory or cache folder (default)).
    /// Example
    /// ```rust
    ///     let template = TypstTemplateCollection::new(vec![font])
    ///         .with_package_file_resolver(None);
    /// ```
    pub fn with_package_file_resolver(self) -> Self {
        use package_resolver::PackageResolverBuilder;
        let file_resolver = PackageResolverBuilder::builder()
            .with_file_system_cache()
            .build()
            .into_cached();
        self.add_file_resolver(file_resolver)
    }

    pub fn build(self) -> TypstEngine<T> {
        let TypstTemplateEngineBuilder {
            template,
            inject_location,
            file_resolvers,
            comemo_evict_max_age,
            fonts,
            #[cfg(feature = "typst-kit-fonts")]
            typst_kit_font_options,
        } = self;

        let mut book = FontBook::new();
        if let Some(fonts) = &fonts {
            for f in fonts {
                book.push(f.info().clone());
            }
        }

        #[allow(unused_mut)]
        let mut fonts: Vec<_> = fonts.into_iter().flatten().map(FontEnum::Font).collect();

        #[cfg(feature = "typst-kit-fonts")]
        if let Some(typst_kit_font_options) = typst_kit_font_options {
            let typst_kit_options::TypstKitFontOptions {
                include_system_fonts,
                include_dirs,
                #[cfg(feature = "typst-kit-embed-fonts")]
                include_embedded_fonts,
            } = typst_kit_font_options;
            let mut searcher = typst_kit::fonts::Fonts::searcher();
            #[cfg(feature = "typst-kit-embed-fonts")]
            searcher.include_embedded_fonts(include_embedded_fonts);
            let typst_kit::fonts::Fonts {
                book: typst_kit_book,
                fonts: typst_kit_fonts,
            } = searcher
                .include_system_fonts(include_system_fonts)
                .search_with(include_dirs);
            let len = typst_kit_fonts.len();
            let font_slots = typst_kit_fonts.into_iter().map(FontEnum::FontSlot);
            if fonts.is_empty() {
                book = typst_kit_book;
                fonts = font_slots.collect();
            } else {
                for i in 0..len {
                    let Some(info) = typst_kit_book.info(i) else {
                        break;
                    };
                    book.push(info.clone());
                }
                fonts.extend(font_slots);
            }
        }

        TypstEngine {
            template,
            inject_location,
            file_resolvers,
            comemo_evict_max_age,
            library: Default::default(),
            book: LazyHash::new(book),
            fonts,
        }
    }
}

struct TypstWorld<'a> {
    library: Cow<'a, LazyHash<Library>>,
    main_source_id: FileId,
    now: DateTime<Utc>,
    book: &'a LazyHash<FontBook>,
    file_resolvers: &'a [Box<dyn FileResolver + Send + Sync + 'static>],
    fonts: &'a [FontEnum],
}

impl typst::World for TypstWorld<'_> {
    fn library(&self) -> &LazyHash<Library> {
        self.library.as_ref()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.book
    }

    fn main(&self) -> FileId {
        self.main_source_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let Self { file_resolvers, .. } = *self;
        let mut last_error = not_found(id);
        for file_resolver in file_resolvers {
            match file_resolver.resolve_source(id) {
                Ok(source) => return Ok(source.into_owned()),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let Self { file_resolvers, .. } = *self;
        let mut last_error = not_found(id);
        for file_resolver in file_resolvers {
            match file_resolver.resolve_binary(id) {
                Ok(file) => return Ok(file.into_owned()),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    fn font(&self, id: usize) -> Option<Font> {
        self.fonts[id].get()
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

#[derive(Debug)]
pub enum FontEnum {
    Font(Font),
    #[cfg(feature = "typst-kit-fonts")]
    FontSlot(typst_kit::fonts::FontSlot),
}

impl FontEnum {
    pub fn get(&self) -> Option<Font> {
        match self {
            FontEnum::Font(font) => Some(font.clone()),
            #[cfg(feature = "typst-kit-fonts")]
            FontEnum::FontSlot(font_slot) => font_slot.get(),
        }
    }
}
