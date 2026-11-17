#![warn(missing_docs)]
//! Small wrapper around [Typst](https://github.com/typst/typst) that makes it easier to use it as a templating engine.
//!
//! See the [repository README](https://github.com/Relacibo/typst-as-lib) for usage examples.
//!
//! Inspired by <https://github.com/tfachmann/typst-as-library/blob/main/src/lib.rs>
use std::borrow::Cow;
use std::ops::Deref;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::diagnostic::SourceDiagnostics;
use crate::time::{SystemTimeProvider, TimeProvider};
use ::time::OffsetDateTime;
use cached_file_resolver::IntoCachedFileResolver;
use conversions::{IntoBytes, IntoFileId, IntoFonts, IntoSource};
use file_resolver::{
    FileResolver, FileSystemResolver, MainSourceFileResolver, StaticFileResolver,
    StaticSourceFileResolver,
};
use thiserror::Error;
use typst::diag::{FileError, FileResult, HintedString, Warned};
use typst::foundations::Output;
use typst::foundations::{Bytes, Datetime, Dict, Module, Scope, Value};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt};
use util::not_found;

/// Caching wrapper for file resolvers.
pub mod cached_file_resolver;
/// Type conversion traits for Typst types.
pub mod conversions;
/// Diagnostics with resolvable source locations.
pub mod diagnostic;
/// File resolution for Typst sources and binaries.
pub mod file_resolver;
/// Time providers for Typst's `datetime.today()`.
pub mod time;
pub(crate) mod util;

#[cfg(all(feature = "packages", any(feature = "ureq", feature = "reqwest")))]
/// Package resolution and downloading from the Typst package repository.
pub mod package_resolver;

#[cfg(feature = "typst-kit-fonts")]
/// Configuration options for `typst-kit` font searching.
pub mod typst_kit_options;

/// Main entry point for compiling Typst documents.
///
/// Use [`TypstEngine::builder()`] to construct an instance. You can optionally set a
/// main file with [`main_file()`](TypstTemplateEngineBuilder::main_file), which allows
/// compiling without specifying the file ID each time.
///
/// # Examples
///
/// With main file (compile without file ID):
///
/// ```rust,no_run
/// # use typst_as_lib::TypstEngine;
/// # use typst_layout::PagedDocument;
/// static TEMPLATE: &str = "Hello World!";
/// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
///
/// let engine = TypstEngine::builder()
///     .main_file(TEMPLATE)
///     .fonts([FONT])
///     .build();
///
/// // Compile the main file directly
/// let doc: PagedDocument = engine.compile().output.expect("Compilation failed");
/// ```
///
/// Without main file (must provide file ID):
///
/// ```rust,no_run
/// # use typst_as_lib::TypstEngine;
/// # use typst_layout::PagedDocument;
/// static TEMPLATE: &str = "Hello World!";
/// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
///
/// let engine = TypstEngine::builder()
///     .fonts([FONT])
///     .with_static_source_file_resolver([("template.typ", TEMPLATE)])
///     .build();
///
/// // Must specify file ID for each compile
/// let doc: PagedDocument = engine.compile("template.typ").output.expect("Compilation failed");
/// ```
///
/// See also: [Examples directory](https://github.com/Relacibo/typst-as-lib/tree/main/examples)
pub struct TypstEngine<T = TypstTemplateCollection> {
    template: T,
    book: LazyHash<FontBook>,
    inject_location: Option<InjectLocation>,
    file_resolvers: Vec<Box<dyn FileResolver + Send + Sync + 'static>>,
    library: LazyHash<Library>,
    comemo_evict_max_age: Option<usize>,
    fonts: Vec<FontEnum>,
    time_provider: Box<dyn TimeProvider>,
}

/// Type state indicating no main file is set.
#[derive(Debug, Clone, Copy)]
pub struct TypstTemplateCollection;

/// Type state indicating a main file has been set.
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
        Doc: Output,
    {
        let mut builder = TypstWorldBuilder::new(self, main_source_id);
        if let Some(inputs) = inputs {
            builder = builder.with_inputs(inputs);
        }
        let world = match builder.build() {
            Ok(world) => world,
            Err(err) => {
                return Warned {
                    output: Err(err),
                    warnings: Default::default(),
                };
            }
        };
        let Warned { output, warnings } = typst::compile(&world);
        if let Some(max_age) = self.comemo_evict_max_age {
            comemo::evict(max_age);
        }
        let output = output
            .map_err(|diags| TypstAsLibError::TypstSource(SourceDiagnostics::new(&world, diags)));
        Warned { output, warnings }
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
            let input_dict: Dict = input.into();
            if let Some(module_value) = global.get_mut(module_name) {
                let module_value = module_value.write()?;
                if let Value::Module(module) = module_value {
                    let scope = module.scope_mut();
                    if let Some(target) = scope.get_mut(value_name) {
                        // Override existing field
                        *target.write()? = Value::Dict(input_dict);
                    } else {
                        // Write new field into existing module scope
                        scope.define(value_name, input_dict);
                    }
                } else {
                    // Override existing non module value
                    let mut scope = Scope::deduplicating();
                    scope.define(value_name, input_dict);
                    let module = Module::new(module_name, scope);
                    *module_value = Value::Module(module);
                }
            } else {
                // Create new module and field
                let mut scope = Scope::deduplicating();
                scope.define(value_name, input_dict);
                let module = Module::new(module_name, scope);
                global.define(module_name, module);
            }
        }
        Ok(LazyHash::new(lib))
    }
}

impl TypstEngine<TypstTemplateCollection> {
    /// Creates a new builder for configuring a [`TypstEngine`].
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .fonts([FONT])
    ///     .build();
    /// ```
    pub fn builder() -> TypstTemplateEngineBuilder {
        TypstTemplateEngineBuilder::default()
    }
}

impl TypstEngine<TypstTemplateCollection> {
    /// Compiles a Typst document with input data injected as `sys.inputs`.
    ///
    /// The input will be available in Typst scripts via `#import sys: inputs`.
    ///
    /// Compilation errors carry a [`SourceDiagnostics`] with resolvable
    /// locations; warnings stay raw. See the [`diagnostic`] module.
    ///
    /// To change the injection location, use [`custom_inject_location()`](TypstTemplateEngineBuilder::custom_inject_location).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// # use typst::foundations::{Dict, IntoValue};
    /// # use typst_layout::PagedDocument;
    /// static TEMPLATE: &str = "#import sys: inputs\n#inputs.name";
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .fonts([FONT])
    ///     .with_static_source_file_resolver([("main.typ", TEMPLATE)])
    ///     .build();
    ///
    /// let mut inputs = Dict::new();
    /// inputs.insert("name".into(), "World".into_value());
    ///
    /// let doc: PagedDocument = engine.compile_with_input("main.typ", inputs)
    ///     .output
    ///     .expect("Compilation failed");
    /// ```
    ///
    /// See also: [resolve_static.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_static.rs)
    pub fn compile_with_input<F, D, Doc>(
        &self,
        main_source_id: F,
        inputs: D,
    ) -> Warned<Result<Doc, TypstAsLibError>>
    where
        F: IntoFileId,
        D: Into<Dict>,
        Doc: Output,
    {
        self.do_compile(main_source_id.into_file_id(), Some(inputs.into()))
    }

    /// Compiles a Typst document without input data.
    ///
    /// Compilation errors carry a [`SourceDiagnostics`] with resolvable
    /// locations; warnings stay raw. See the [`diagnostic`] module.
    pub fn compile<F, Doc>(&self, main_source_id: F) -> Warned<Result<Doc, TypstAsLibError>>
    where
        F: IntoFileId,
        Doc: Output,
    {
        self.do_compile(main_source_id.into_file_id(), None)
    }

    /// Returns a [`TypstWorldBuilder`] for constructing a [`TypstWorld`] bound to a specific file.
    ///
    /// This is an advanced low-level API. The caller is responsible for driving compilation
    /// (e.g. via `typst::compile`) and for managing the `comemo` cache afterwards.
    /// No cache eviction is performed automatically — use [`with_world`](Self::with_world)
    /// if you want eviction handled for you.
    ///
    /// # Example
    /// ```rust,ignore
    /// # use typst_as_lib::TypstEngine;
    /// let engine = TypstEngine::builder().build();
    ///
    /// let world = engine.world_builder("/main.typ")
    ///     .with_inputs(my_inputs)
    ///     .build()?;
    /// let doc = typst::compile(&world).output.expect("Failed");
    /// comemo::evict(30); // caller manages cache eviction
    /// ```
    pub fn world_builder<I>(
        &self,
        main_source_id: I,
    ) -> TypstWorldBuilder<'_, TypstTemplateCollection>
    where
        I: IntoFileId,
    {
        TypstWorldBuilder::new(self, main_source_id.into_file_id())
    }

    /// Execute a closure with a [`TypstWorld`] for a specific file,
    /// optionally injecting custom inputs.
    ///
    /// Runs the `comemo` cache eviction after the closure returns.
    /// For full control use [`world_builder`](Self::world_builder) instead.
    ///
    /// # Example
    /// ```rust,ignore
    /// # use typst_as_lib::TypstEngine;
    /// let engine = TypstEngine::builder().build();
    ///
    /// let pdf_bytes = engine.with_world("/main.typ", |world| {
    ///     let doc = typst::compile(world).output.expect("Failed");
    ///     typst_pdf::pdf(&doc, Default::default()).expect("Failed")
    /// }).unwrap();
    ///
    /// // With inputs:
    /// let pdf_bytes = engine.with_world("/main.typ", |world| { ... }).unwrap();
    /// ```
    pub fn with_world<F, I, R>(&self, main_source_id: I, f: F) -> Result<R, TypstAsLibError>
    where
        I: IntoFileId,
        F: FnOnce(&TypstWorld<'_>) -> R,
    {
        let world = self.world_builder(main_source_id).build()?;
        let result = f(&world);
        if let Some(max_age) = self.comemo_evict_max_age {
            comemo::evict(max_age);
        }
        Ok(result)
    }
}

impl TypstEngine<TypstTemplateMainFile> {
    /// Compiles the main file with input data injected as `sys.inputs`.
    ///
    /// The input will be available in Typst scripts via `#import sys: inputs`.
    ///
    /// Compilation errors carry a [`SourceDiagnostics`] with resolvable
    /// locations; warnings stay raw. See the [`diagnostic`] module.
    ///
    /// To change the injection location, use [`custom_inject_location()`](TypstTemplateEngineBuilder::custom_inject_location).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// # use typst::foundations::{Dict, IntoValue};
    /// # use typst_layout::PagedDocument;
    /// static TEMPLATE: &str = "#import sys: inputs\nHello #inputs.name!";
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .main_file(TEMPLATE)
    ///     .fonts([FONT])
    ///     .build();
    ///
    /// let mut inputs = Dict::new();
    /// inputs.insert("name".into(), "World".into_value());
    ///
    /// let doc: PagedDocument = engine.compile_with_input(inputs)
    ///     .output
    ///     .expect("Compilation failed");
    /// ```
    ///
    /// See also: [small_example.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/small_example.rs)
    pub fn compile_with_input<D, Doc>(&self, inputs: D) -> Warned<Result<Doc, TypstAsLibError>>
    where
        D: Into<Dict>,
        Doc: Output,
    {
        let TypstTemplateMainFile { source_id } = self.template;
        self.do_compile(source_id, Some(inputs.into()))
    }

    /// Compiles the main file without input data.
    ///
    /// Compilation errors carry a [`SourceDiagnostics`] with resolvable
    /// locations; warnings stay raw. See the [`diagnostic`] module.
    pub fn compile<Doc>(&self) -> Warned<Result<Doc, TypstAsLibError>>
    where
        Doc: Output,
    {
        let TypstTemplateMainFile { source_id } = self.template;
        self.do_compile(source_id, None)
    }

    /// Returns a [`TypstWorldBuilder`] using the engine's pre-configured main file.
    ///
    /// This is an advanced low-level API. The caller is responsible for driving compilation
    /// (e.g. via `typst::compile`) and for managing the `comemo` cache afterwards.
    /// No cache eviction is performed automatically — use [`with_world`](Self::with_world)
    /// if you want eviction handled for you.
    ///
    /// # Example
    /// ```rust,ignore
    /// # use typst_as_lib::TypstEngine;
    /// let engine = TypstEngine::builder().main_file("= Hello").build();
    ///
    /// let world = engine.world_builder()
    ///     .with_inputs(my_inputs)
    ///     .build()?;
    /// let doc = typst::compile(&world).output.expect("Failed");
    /// comemo::evict(30); // caller manages cache eviction
    /// ```
    pub fn world_builder(&self) -> TypstWorldBuilder<'_, TypstTemplateMainFile> {
        let TypstTemplateMainFile { source_id } = self.template;
        TypstWorldBuilder::new(self, source_id)
    }

    /// Execute a closure with a [`TypstWorld`] using the engine's pre-configured main file,
    /// optionally injecting custom inputs.
    ///
    /// Runs the `comemo` cache eviction after the closure returns.
    /// For full control use [`world_builder`](Self::world_builder) instead.
    ///
    /// # Example
    /// ```rust,ignore
    /// # use typst_as_lib::TypstEngine;
    /// let engine = TypstEngine::builder().main_file("= Hello").build();
    ///
    /// let pdf_bytes = engine.with_world(|world| {
    ///     let doc = typst::compile(world).output.expect("Failed");
    ///     typst_pdf::pdf(&doc, Default::default()).expect("Failed")
    /// }).unwrap();
    /// ```
    pub fn with_world<F, R>(&self, f: F) -> Result<R, TypstAsLibError>
    where
        F: FnOnce(&TypstWorld<'_>) -> R,
    {
        let world = self.world_builder().build()?;
        let result = f(&world);
        if let Some(max_age) = self.comemo_evict_max_age {
            comemo::evict(max_age);
        }
        Ok(result)
    }
}

/// Builder for constructing a [`TypstEngine`].
pub struct TypstTemplateEngineBuilder<T = TypstTemplateCollection> {
    template: T,
    inject_location: Option<InjectLocation>,
    file_resolvers: Vec<Box<dyn FileResolver + Send + Sync + 'static>>,
    comemo_evict_max_age: Option<usize>,
    fonts: Option<Vec<Font>>,
    #[cfg(feature = "typst-kit-fonts")]
    typst_kit_font_options: Option<typst_kit_options::TypstKitFontOptions>,
    time_provider: Box<dyn TimeProvider>,
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
            time_provider: Box::new(SystemTimeProvider),
        }
    }
}

impl TypstTemplateEngineBuilder<TypstTemplateCollection> {
    /// Sets the main file for compilation.
    ///
    /// This is optional. If not set, you must provide a file ID on each compile call.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// # use typst_layout::PagedDocument;
    /// static TEMPLATE: &str = "Hello World!";
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .main_file(TEMPLATE)
    ///     .fonts([FONT])
    ///     .build();
    ///
    /// let doc: PagedDocument = engine.compile().output.expect("Compilation failed");
    /// ```
    ///
    /// See also: [small_example.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/small_example.rs)
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
            time_provider,
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
            time_provider,
        }
    }
}

impl<T> TypstTemplateEngineBuilder<T> {
    /// Customizes where input data is injected in the Typst environment.
    ///
    /// By default, inputs are available as `sys.inputs`.
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

    /// Adds fonts for rendering.
    ///
    /// Accepts font data as `&[u8]`, `Vec<u8>`, `Bytes`, or `Font`.
    ///
    /// For automatic system font discovery, see `typst-kit-fonts` feature.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .fonts([FONT])
    ///     .build();
    /// ```
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

    /// Enables system font discovery using `typst-kit`.
    ///
    /// See [`typst_kit_options::TypstKitFontOptions`] for configuration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// # use typst_as_lib::typst_kit_options::TypstKitFontOptions;
    /// let engine = TypstEngine::builder()
    ///     .search_fonts_with(TypstKitFontOptions::default())
    ///     .build();
    /// ```
    ///
    /// See also: [font_searcher.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/font_searcher.rs)
    #[cfg(feature = "typst-kit-fonts")]
    pub fn search_fonts_with(mut self, options: typst_kit_options::TypstKitFontOptions) -> Self {
        self.typst_kit_font_options = Some(options);
        self
    }

    /// Adds a custom file resolver.
    ///
    /// Resolvers are tried in order until one successfully resolves the file.
    ///
    /// To share a resolver (e.g. one package cache) between multiple engines,
    /// wrap it in `Arc` — [`Arc`](std::sync::Arc) implements [`FileResolver`]
    /// via a blanket impl.
    pub fn add_file_resolver<F>(mut self, file_resolver: F) -> Self
    where
        F: FileResolver + Send + Sync + 'static,
    {
        self.file_resolvers.push(Box::new(file_resolver));
        self
    }

    /// Adds static source files embedded in memory.
    ///
    /// Accepts sources as `&str`, `String`, `(&str, &str)` (path, content),
    /// `(FileId, &str)`, or `Source`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// # use typst_layout::PagedDocument;
    /// static MAIN: &str = "#import \"lib.typ\": greet\n#greet()";
    /// static LIB: &str = "#let greet() = [Hello World!]";
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .fonts([FONT])
    ///     .with_static_source_file_resolver([
    ///         ("main.typ", MAIN),
    ///         ("lib.typ", LIB),
    ///     ])
    ///     .build();
    ///
    /// let doc: PagedDocument = engine.compile("main.typ").output.expect("Compilation failed");
    /// ```
    ///
    /// See also: [resolve_static.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_static.rs)
    pub fn with_static_source_file_resolver<IS, S>(self, sources: IS) -> Self
    where
        IS: IntoIterator<Item = S>,
        S: IntoSource,
    {
        self.add_file_resolver(StaticSourceFileResolver::new(sources))
    }

    /// Adds static binary files embedded in memory (e.g., images).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// static TEMPLATE: &str = r#"#image("logo.png")"#;
    /// static LOGO: &[u8] = include_bytes!("../examples/templates/images/typst.png");
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .main_file(TEMPLATE)
    ///     .fonts([FONT])
    ///     .with_static_file_resolver([("logo.png", LOGO)])
    ///     .build();
    /// ```
    ///
    /// See also: [resolve_static.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_static.rs)
    pub fn with_static_file_resolver<IB, F, B>(self, binaries: IB) -> Self
    where
        IB: IntoIterator<Item = (F, B)>,
        F: IntoFileId,
        B: IntoBytes,
    {
        self.add_file_resolver(StaticFileResolver::new(binaries))
    }

    /// Enables loading files from the file system.
    ///
    /// Files are resolved relative to `root`. Files outside of `root` cannot be accessed.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// static TEMPLATE: &str = r#"#include "header.typ""#;
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .main_file(TEMPLATE)
    ///     .fonts([FONT])
    ///     .with_file_system_resolver("./templates")
    ///     .build();
    /// ```
    ///
    /// See also: [resolve_packages.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_packages.rs)
    pub fn with_file_system_resolver<P>(self, root: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.add_file_resolver(FileSystemResolver::new(root.into()).into_cached())
    }

    /// Sets the maximum age for comemo cache eviction after compilation.
    ///
    /// Default is `Some(0)`, which evicts after each compilation. For hosts
    /// that recompile frequently with few changes (editors, playgrounds),
    /// consider a value like `Some(10)`: entries that produce cache hits keep
    /// their age at zero and stay cached, while stale entries rotate out —
    /// keeping memory bounded while retaining incremental reuse.
    pub fn comemo_evict_max_age(mut self, comemo_evict_max_age: Option<usize>) -> Self {
        self.comemo_evict_max_age = comemo_evict_max_age;
        self
    }

    /// Sets the [`TimeProvider`] that determines the time `datetime.today()`
    /// returns during compilation.
    ///
    /// Defaults to [`SystemTimeProvider`] (the system clock). The provider is
    /// queried once per compilation, so all `datetime.today()` calls within
    /// one document see the same time. It is used by every compilation
    /// method: [`compile`](TypstEngine::compile),
    /// [`compile_with_input`](TypstEngine::compile_with_input),
    /// [`with_world`](TypstEngine::with_world) and
    /// [`world_builder`](TypstEngine::world_builder).
    ///
    /// To share a stateful clock (e.g. a steerable test clock) across
    /// engines, pass `Arc::clone(&clock)` — [`Arc`](std::sync::Arc)
    /// implements [`TimeProvider`] via a blanket impl, as does [`Box`].
    ///
    /// To override the time for a single compilation only, see
    /// [`with_fixed_time`](TypstWorldBuilder::with_fixed_time).
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typst_as_lib::{TypstEngine, time::FixedTimeProvider};
    /// # use std::time::{Duration, SystemTime};
    /// static TEMPLATE: &str = "#datetime.today().display()";
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_718_452_800);
    /// let engine = TypstEngine::builder()
    ///     .main_file(TEMPLATE)
    ///     .fonts([FONT])
    ///     .time_provider(FixedTimeProvider::new(fixed))
    ///     .build();
    ///
    /// let world = engine.world_builder().build().unwrap();
    /// ```
    pub fn time_provider<P: TimeProvider + 'static>(mut self, provider: P) -> Self {
        self.time_provider = Box::new(provider);
        self
    }

    /// Enables downloading packages from the Typst package repository.
    ///
    /// Packages are cached on the file system for reuse.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use typst_as_lib::TypstEngine;
    /// static TEMPLATE: &str = r#"#import "@preview/example:0.1.0": *"#;
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .main_file(TEMPLATE)
    ///     .fonts([FONT])
    ///     .with_package_file_resolver()
    ///     .build();
    /// ```
    ///
    /// See also: [resolve_packages.rs](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_packages.rs)
    #[cfg(all(feature = "packages", any(feature = "ureq", feature = "reqwest")))]
    pub fn with_package_file_resolver(self) -> Self {
        use package_resolver::PackageResolver;
        let file_resolver = PackageResolver::builder()
            .with_file_system_cache()
            .build()
            .into_cached();
        self.add_file_resolver(file_resolver)
    }

    /// Builds the [`TypstEngine`] with the configured options.
    pub fn build(self) -> TypstEngine<T> {
        let TypstTemplateEngineBuilder {
            template,
            inject_location,
            file_resolvers,
            comemo_evict_max_age,
            fonts,
            #[cfg(feature = "typst-kit-fonts")]
            typst_kit_font_options,
            time_provider,
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
            let mut tk_book = typst::text::FontBook::new();
            let mut tk_fonts: Vec<FontEnum> = Vec::new();

            #[cfg(feature = "typst-kit-embed-fonts")]
            if include_embedded_fonts {
                for (font, info) in typst_kit::fonts::embedded() {
                    tk_book.push(info);
                    tk_fonts.push(FontEnum::Font(font));
                }
            }

            if include_system_fonts {
                for (font_path, info) in typst_kit::fonts::system() {
                    tk_book.push(info);
                    tk_fonts.push(FontEnum::FontPath(font_path));
                }
            }
            for dir in &include_dirs {
                for (font_path, info) in typst_kit::fonts::scan(dir) {
                    tk_book.push(info);
                    tk_fonts.push(FontEnum::FontPath(font_path));
                }
            }

            let len = tk_fonts.len();
            if fonts.is_empty() {
                book = tk_book;
                fonts = tk_fonts;
            } else {
                for i in 0..len {
                    let Some(info) = tk_book.info(i) else {
                        break;
                    };
                    book.push(info.clone());
                }
                fonts.extend(tk_fonts);
            }
        }

        #[cfg(not(feature = "typst-html"))]
        let library = typst::Library::builder().build();

        #[cfg(feature = "typst-html")]
        let library = typst::Library::builder()
            .with_features([typst::Feature::Html].into_iter().collect())
            .build();

        TypstEngine {
            template,
            inject_location,
            file_resolvers,
            comemo_evict_max_age,
            library: LazyHash::new(library),
            book: LazyHash::new(book),
            fonts,
            time_provider,
        }
    }
}

/// Converts a `SystemTime` to an `OffsetDateTime` in UTC. Returns `None`
/// when the value lies outside the range supported by the `time` crate
/// (years -9999 to 9999), e.g. for a misconfigured [`TimeProvider`].
fn system_time_to_datetime(now: SystemTime) -> Option<OffsetDateTime> {
    match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => {
            let duration = ::time::Duration::try_from(duration).ok()?;
            OffsetDateTime::UNIX_EPOCH.checked_add(duration)
        }
        Err(_) => {
            let duration =
                ::time::Duration::try_from(SystemTime::UNIX_EPOCH.duration_since(now).ok()?)
                    .ok()?;
            OffsetDateTime::UNIX_EPOCH.checked_sub(duration)
        }
    }
}

/// The Typst world instance used for compilation.
///
/// Borrows its configuration from a [`TypstEngine`]. Constructed via
/// [`TypstEngine::world_builder`] or [`TypstEngine::with_world`].
pub struct TypstWorld<'a> {
    library: Cow<'a, LazyHash<Library>>,
    main_source_id: FileId,
    now: Option<OffsetDateTime>,
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

    fn today(&self, offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        let mut now = self.now?;
        if let Some(offset) = offset {
            // Apply the full offset, not just whole hours — the duration may
            // contain minutes/seconds and can be negative. Out-of-range
            // offsets resolve to `None`, which Typst reports as a normal
            // error instead of panicking.
            now = now.checked_add(offset.into())?;
        }
        let date = now.date();
        Datetime::from_ymd(date.year(), u8::from(date.month()), date.day())
    }
}

/// Builder for constructing a [`TypstWorld`] from a [`TypstEngine`].
///
/// Obtained via [`TypstEngine::world_builder`]. Call [`with_inputs`](Self::with_inputs)
/// or [`with_fixed_time`](Self::with_fixed_time) optionally, then [`build`](Self::build) to get the world.
pub struct TypstWorldBuilder<'a, T> {
    engine: &'a TypstEngine<T>,
    main_source_id: FileId,
    inputs: Option<Dict>,
    now_override: Option<SystemTime>,
}

impl<'a, T> TypstWorldBuilder<'a, T> {
    fn new(engine: &'a TypstEngine<T>, main_source_id: FileId) -> Self {
        Self {
            engine,
            main_source_id,
            inputs: None,
            now_override: None,
        }
    }

    /// Injects a `Dict` as `sys.inputs` into the compiled document.
    pub fn with_inputs<D: Into<Dict>>(mut self, inputs: D) -> Self {
        self.inputs = Some(inputs.into());
        self
    }

    /// Overrides the time `datetime.today()` returns for this compilation,
    /// regardless of the [`TimeProvider`] configured on the [`TypstEngine`].
    ///
    /// Useful for reproducible output, e.g. snapshot tests or deterministic
    /// builds of documents that use `datetime.today()`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typst_as_lib::TypstEngine;
    /// # use std::time::{Duration, SystemTime};
    /// static TEMPLATE: &str = "#datetime.today().display()";
    /// static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
    ///
    /// let engine = TypstEngine::builder()
    ///     .main_file(TEMPLATE)
    ///     .fonts([FONT])
    ///     .build();
    ///
    /// let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_718_452_800);
    /// let world = engine.world_builder().with_fixed_time(fixed).build().unwrap();
    /// ```
    pub fn with_fixed_time(mut self, now: SystemTime) -> Self {
        self.now_override = Some(now);
        self
    }

    /// Builds the [`TypstWorld`]. Returns an error if input injection fails.
    pub fn build(self) -> Result<TypstWorld<'a>, TypstAsLibError> {
        let library = if let Some(inputs) = self.inputs {
            Cow::Owned(self.engine.create_injected_library(inputs)?)
        } else {
            Cow::Borrowed(&self.engine.library)
        };

        let now = self
            .now_override
            .unwrap_or_else(|| self.engine.time_provider.now());

        Ok(TypstWorld {
            main_source_id: self.main_source_id,
            library,
            now: system_time_to_datetime(now),
            file_resolvers: &self.engine.file_resolvers,
            book: &self.engine.book,
            fonts: &self.engine.fonts,
        })
    }
}

#[derive(Debug, Clone)]
struct InjectLocation {
    module_name: &'static str,
    value_name: &'static str,
}

/// Errors that can occur when using typst-as-lib.
#[derive(Debug, Clone, Error)]
pub enum TypstAsLibError {
    /// Errors from Typst source compilation, together with the sources
    /// referenced by their spans, so that locations remain resolvable
    /// without a world. See [`diagnostic::SourceDiagnostics`].
    #[error("{0}")]
    TypstSource(SourceDiagnostics),
    /// Errors from file operations.
    #[error("Typst file error: {0}")]
    TypstFile(#[from] FileError),
    /// The specified main source file was not found.
    #[error("Source file does not exist in collection: {0:?}")]
    MainSourceFileDoesNotExist(FileId),
    /// Errors with additional hints from Typst.
    #[error("Typst hinted String: {0:?}")]
    HintedString(HintedString),
    /// Other unspecified errors.
    #[error("Unspecified: {0}!")]
    Unspecified(ecow::EcoString),
}

impl From<HintedString> for TypstAsLibError {
    fn from(value: HintedString) -> Self {
        TypstAsLibError::HintedString(value)
    }
}

impl From<ecow::EcoString> for TypstAsLibError {
    fn from(value: ecow::EcoString) -> Self {
        TypstAsLibError::Unspecified(value)
    }
}

/// Wrapper for different font types.
#[derive(Debug)]
pub enum FontEnum {
    /// A directly loaded font.
    Font(Font),
    /// A lazy font slot from typst-kit.
    #[cfg(feature = "typst-kit-fonts")]
    FontPath(typst_kit::fonts::FontPath),
}

impl FontEnum {
    /// Retrieves the font, loading it if necessary.
    pub fn get(&self) -> Option<Font> {
        match self {
            FontEnum::Font(font) => Some(font.clone()),
            #[cfg(feature = "typst-kit-fonts")]
            FontEnum::FontPath(font_path) => {
                use typst_kit::fonts::FontSource;
                font_path.load()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::SourceDiagnostics;
    use crate::time::FixedTimeProvider;
    use ecow::EcoVec;
    use std::sync::Arc;
    use typst::layout::FrameItem;
    use typst::syntax::Span;
    use typst_layout::PagedDocument;

    /// Fixed instant as seconds since the Unix epoch.
    fn fixed_time(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    /// Collects the text of all pages of a compiled document.
    fn page_text(doc: &PagedDocument) -> String {
        let mut text = String::new();
        for page in doc.pages() {
            for (_, item) in page.frame.items() {
                if let FrameItem::Text(text_item) = item {
                    text.push_str(&text_item.text);
                }
            }
        }
        text
    }

    fn engine_with_time(
        template: &str,
        provider: impl TimeProvider + 'static,
    ) -> TypstEngine<TypstTemplateMainFile> {
        static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
        TypstEngine::builder()
            .main_file(template)
            .fonts([FONT])
            .time_provider(provider)
            .build()
    }

    #[test]
    fn engine_time_provider_used_by_compile() {
        // compile() (without the world builder) must use the engine's provider.
        // 1_718_452_800 = 2024-06-15T12:00:00Z
        let engine = engine_with_time(
            "#datetime.today().display()",
            FixedTimeProvider::new(fixed_time(1_718_452_800)),
        );
        let doc: PagedDocument = engine.compile().output.unwrap();
        assert_eq!(page_text(&doc), "2024-06-15");
    }

    #[test]
    fn world_build_uses_engine_time_provider() {
        let engine = engine_with_time(
            "#datetime.today().display()",
            FixedTimeProvider::new(fixed_time(1_718_452_800)),
        );
        let world = engine.world_builder().build().unwrap();
        let doc: PagedDocument = typst::compile(&world).output.unwrap();
        assert_eq!(page_text(&doc), "2024-06-15");
    }

    #[test]
    fn with_fixed_time_overrides_engine_time_provider() {
        let engine = engine_with_time(
            "#datetime.today().display()",
            FixedTimeProvider::new(fixed_time(1_718_452_800)),
        );
        let world = engine
            .world_builder()
            .with_fixed_time(fixed_time(1_704_067_200)) // 2024-01-01T00:00:00Z
            .build()
            .unwrap();
        let doc: PagedDocument = typst::compile(&world).output.unwrap();
        assert_eq!(page_text(&doc), "2024-01-01");
    }

    #[test]
    fn arc_wrapped_time_provider_can_be_shared() {
        let clock = Arc::new(FixedTimeProvider::new(fixed_time(1_718_452_800)));
        let engine = engine_with_time("#datetime.today().display()", Arc::clone(&clock));
        let doc: PagedDocument = engine.compile().output.unwrap();
        assert_eq!(page_text(&doc), "2024-06-15");
        drop(engine);
        // The caller keeps its handle after the engine is gone.
        assert_eq!(clock.now(), fixed_time(1_718_452_800));
    }

    #[test]
    fn today_offset_overflow_is_a_normal_error() {
        // A template-controlled out-of-range offset must not panic the host;
        // Typst reports it as a regular compile error instead.
        let engine = engine_with_time(
            "#datetime.today(offset: duration(days: 10000000)).display()",
            FixedTimeProvider::new(fixed_time(1_718_452_800)),
        );
        let err = engine
            .compile::<PagedDocument>()
            .output
            .expect_err("out-of-range offset must fail compilation, not panic");
        let TypstAsLibError::TypstSource(diags) = err else {
            panic!("expected TypstSource diagnostics");
        };
        assert!(
            diags
                .iter()
                .any(|diag| diag.message.contains("unable to get the current date"))
        );
    }

    #[test]
    fn out_of_range_provider_time_is_a_normal_error() {
        // ~15844 years after the Unix epoch (year ~17814) — beyond the
        // `time` crate's year-9999 limit, but still representable as a
        // `SystemTime` on all platforms (Windows' FILETIME range ends
        // around year 30000, so larger values would already panic when
        // constructing the `SystemTime` in this test). Building the world
        // must not panic; Typst reports a normal error.
        let engine = engine_with_time(
            "#datetime.today().display()",
            FixedTimeProvider::new(fixed_time(500_000_000_000)),
        );
        let err = engine
            .compile::<PagedDocument>()
            .output
            .expect_err("out-of-range provider time must fail compilation, not panic");
        let TypstAsLibError::TypstSource(diags) = err else {
            panic!("expected TypstSource diagnostics");
        };
        assert!(
            diags
                .iter()
                .any(|diag| diag.message.contains("unable to get the current date"))
        );
    }

    #[test]
    fn pre_epoch_provider_time_is_supported() {
        // 1969-12-31 — before the Unix epoch, but within the `time` range.
        let engine = engine_with_time(
            "#datetime.today().display()",
            FixedTimeProvider::new(SystemTime::UNIX_EPOCH - std::time::Duration::from_secs(86_400)),
        );
        let doc: PagedDocument = engine.compile::<PagedDocument>().output.unwrap();
        assert_eq!(page_text(&doc), "1969-12-31");
    }

    #[test]
    fn today_applies_full_offset() {
        // 23:30 plus 45 minutes crosses midnight into the next day. The
        // offset must not be truncated to whole hours.
        let engine = engine_with_time(
            "#datetime.today(offset: duration(minutes: 45)).display()",
            FixedTimeProvider::new(fixed_time(1_718_452_800 + 41_400)), // 2024-06-15T23:30:00Z
        );
        let doc: PagedDocument = engine.compile().output.unwrap();
        assert_eq!(page_text(&doc), "2024-06-16");
    }

    #[test]
    fn today_applies_negative_offset() {
        let engine = engine_with_time(
            "#datetime.today(offset: duration(hours: -10)).display()",
            FixedTimeProvider::new(fixed_time(1_718_452_800 - 25_200)), // 2024-06-15T05:00:00Z
        );
        let doc: PagedDocument = engine.compile().output.unwrap();
        assert_eq!(page_text(&doc), "2024-06-14");
    }

    #[test]
    fn arc_wrapped_resolver_can_be_shared() {
        static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
        let shared = Arc::new(StaticSourceFileResolver::new([(
            "main.typ",
            "Shared resolver!",
        )]));

        let engine_a = TypstEngine::builder()
            .fonts([FONT])
            .add_file_resolver(Arc::clone(&shared))
            .build();
        let engine_b = TypstEngine::builder()
            .fonts([FONT])
            .add_file_resolver(Arc::clone(&shared))
            .build();

        let doc_a: PagedDocument = engine_a.compile("main.typ").output.unwrap();
        let doc_b: PagedDocument = engine_b.compile("main.typ").output.unwrap();
        assert_eq!(page_text(&doc_a), "Shared resolver!");
        assert_eq!(page_text(&doc_b), "Shared resolver!");
    }

    fn engine_with_sources(
        sources: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> TypstEngine<TypstTemplateCollection> {
        static FONT: &[u8] = include_bytes!("../examples/fonts/texgyrecursor-regular.otf");
        TypstEngine::builder()
            .fonts([FONT])
            .with_static_source_file_resolver(sources)
            .build()
    }

    fn compile_error_of(
        engine: &TypstEngine<TypstTemplateCollection>,
        file: &'static str,
    ) -> SourceDiagnostics {
        let err = engine
            .compile::<_, PagedDocument>(file)
            .output
            .expect_err("expected compilation to fail");
        match err {
            TypstAsLibError::TypstSource(diags) => diags,
            other => panic!("expected TypstSource diagnostics, got: {other:?}"),
        }
    }

    #[test]
    fn compile_error_display_contains_location_and_message() {
        let engine = engine_with_sources([("main.typ", "= Hello\n#panic(\"invalid color\")")]);
        let err = engine
            .compile::<_, PagedDocument>("main.typ")
            .output
            .expect_err("expected compilation to fail");
        // The error itself is printable — its Display is routed through the
        // diagnostics report:
        let report = err.to_string();
        assert!(report.contains("error:"), "no severity in report: {report}");
        assert!(
            report.contains("main.typ:2:"),
            "no location in report: {report}"
        );
        assert!(
            report.contains("invalid color"),
            "no message in report: {report}"
        );
    }

    #[test]
    fn imported_file_error_resolves_cause_and_trace() {
        let engine = engine_with_sources([
            ("main.typ", "#import \"lib.typ\": boom\n#boom()"),
            ("lib.typ", "#let boom() = panic(\"kaboom\")"),
        ]);
        let diags = compile_error_of(&engine, "main.typ");
        let report = diags.to_string();
        // The cause resolves inside the imported file...
        assert!(report.contains("lib.typ:1:"), "cause not located: {report}");
        // ...and the traceback points at the call site in the main file.
        assert!(report.contains("traceback:"), "no traceback: {report}");
        assert!(
            report.contains("main.typ:2:"),
            "call site not located: {report}"
        );
        assert!(
            report.contains("while calling"),
            "tracepoint not rendered: {report}"
        );
    }

    #[test]
    fn hints_are_printed() {
        // A length without a unit triggers Typst's "did you mean" hint
        // (the argument expects a length, an integer was passed).
        let engine = engine_with_sources([("main.typ", "#text(size: 10)[a]")]);
        let diags = compile_error_of(&engine, "main.typ");
        let report = diags.to_string();
        assert!(report.contains("hints:"), "no hints section: {report}");
        assert!(
            diags.iter().any(|diag| !diag.hints.is_empty()),
            "expected a diagnostic carrying a hint: {report}"
        );
    }

    #[test]
    fn detached_span_location_is_none() {
        let engine = engine_with_sources([("main.typ", "= Hello")]);
        let world = engine.world_builder("main.typ").build().unwrap();
        let diags = SourceDiagnostics::new(&world, EcoVec::new());
        assert_eq!(diags.location(Span::detached()), None);
    }

    #[test]
    fn raw_diagnostics_accessible_through_deref() {
        let engine = engine_with_sources([("main.typ", "#panic(\"boom\")")]);
        let diags = compile_error_of(&engine, "main.typ");
        // Existing code patterns keep compiling through
        // `Deref<Target = [SourceDiagnostic]>`:
        assert_eq!(diags.len(), 1);
        let messages: Vec<&str> = diags.iter().map(|diag| diag.message.as_str()).collect();
        assert!(messages.iter().any(|message| message.contains("boom")));
    }
}
