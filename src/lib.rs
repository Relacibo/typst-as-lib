#![warn(missing_docs)]
//! Small wrapper around [Typst](https://github.com/typst/typst) that makes it easier to use it as a templating engine.
//!
//! See the [repository README](https://github.com/Relacibo/typst-as-lib) for usage examples.
//!
//! Inspired by <https://github.com/tfachmann/typst-as-library/blob/main/src/lib.rs>
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
use typst::foundations::Output;
use typst::foundations::{Bytes, Datetime, Dict, Module, Scope, Value};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt};
use util::not_found;

use crate::diagnostic::{Diagnostic, Diagnostics};

/// Caching wrapper for file resolvers.
pub mod cached_file_resolver;
/// Type conversion traits for Typst types.
pub mod conversions;
/// Pretty-printing for Typst source diagnostics.
pub mod diagnostic;
/// File resolution for Typst sources and binaries.
pub mod file_resolver;
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
        Warned {
            output: output.map_err(|diags| TypstAsLibError::from_diagnostics(&world, diags)),
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
    /// Default is `Some(0)`, which evicts after each compilation.
    pub fn comemo_evict_max_age(&mut self, comemo_evict_max_age: Option<usize>) -> &mut Self {
        self.comemo_evict_max_age = comemo_evict_max_age;
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

    fn today(&self, offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        let mut now = self.now;
        if let Some(offset) = offset {
            now += Duration::hours(offset.hours() as i64);
        }
        let date = now.date_naive();
        let year = date.year();
        let month = (date.month0() + 1) as u8;
        let day = (date.day0() + 1) as u8;
        Datetime::from_ymd(year, month, day)
    }
}

/// Builder for constructing a [`TypstWorld`] from a [`TypstEngine`].
///
/// Obtained via [`TypstEngine::world_builder`]. Call [`with_inputs`](Self::with_inputs)
/// optionally, then [`build`](Self::build) to get the world.
pub struct TypstWorldBuilder<'a, T> {
    engine: &'a TypstEngine<T>,
    main_source_id: FileId,
    inputs: Option<Dict>,
}

impl<'a, T> TypstWorldBuilder<'a, T> {
    fn new(engine: &'a TypstEngine<T>, main_source_id: FileId) -> Self {
        Self {
            engine,
            main_source_id,
            inputs: None,
        }
    }

    /// Injects a `Dict` as `sys.inputs` into the compiled document.
    pub fn with_inputs<D: Into<Dict>>(mut self, inputs: D) -> Self {
        self.inputs = Some(inputs.into());
        self
    }

    /// Builds the [`TypstWorld`]. Returns an error if input injection fails.
    pub fn build(self) -> Result<TypstWorld<'a>, TypstAsLibError> {
        let library = if let Some(inputs) = self.inputs {
            Cow::Owned(self.engine.create_injected_library(inputs)?)
        } else {
            Cow::Borrowed(&self.engine.library)
        };

        Ok(TypstWorld {
            main_source_id: self.main_source_id,
            library,
            now: Utc::now(),
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
    /// Errors from Typst source compilation.
    #[error("{0}")]
    TypstSource(Diagnostics),
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

impl TypstAsLibError {
    fn from_diagnostics<W: typst::World>(world: &W, diagnostics: EcoVec<SourceDiagnostic>) -> Self {
        TypstAsLibError::TypstSource(Diagnostics(
            diagnostics
                .into_iter()
                .map(|diag| Diagnostic::from_source_diagnostic(world, diag))
                .collect(),
        ))
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
