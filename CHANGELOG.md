# Changelog

## [0.12.1] - 2025-02-20
- Add getter for fonts variable in `TypstTemplateCollection`, with feature `typst-kit-fonts`.
- Removed unnecessary (each font slot has a OnceLock) Arc<Mutex<_>> wrapper around fonts with feature `typst-kit-fonts`.
- Removed unnecessary Error `AquireRwLock`
- Added example `font_searcher`

## [0.12.0] - 2025-02-19

- Remove deprecated `TypstTemplate[Collection]::compile_with_input_fast()`.
- Removed fonts argument from `TypstTemplate[Collection]::new()`. Use `TypstTemplate[Collection]::add_fonts()` to add fonts.
- Added optional feature `fonts` that adds capability to use typst-kit for font resolving.
  - Adds function `TypstTemplate[Collection]::search_fonts_with` that accepts `FontSearcherOptions`.
- Updated to typst version 1.13.0
  - Note: Bytes now have to be casted manually: `Bytes::from(array)` now is `Bytes::new(array.to_vec())`
  - Note: `TypstTemplate[Collection]::compile` now returns `impl Document` as output.

## [0.11.1] - 2024-11-11

- Call `comemo::evict(0)` after each call of `typst::compile()`. Can be configured and turned off.
- Deprecate `TypstTemplate[Collection]::compile_with_input_fast()` as it is not really faster.
- Fix: update Cache of library after changing input

## [0.11.0] - 2024-11-11

- `IntoCachedFileResolver` - wraps the file resolver in an in-memory cache
- Add `TypstTemplate[Collection]::compile_with_input_fast()` that takes a mutable reference to `TypstTemplate[Collection]`
- Inject input to sys: input without needing to reinitialize the whole library every time

## [0.10.0] - 2024-10-19

- Updated Typst dependency to version 0.12.0
- compile functions:
  - `tracer` argument removed
  - Return Type of is now wrapped in `Warned` type
- Added optional in-memory-caching of sources and binary files for
  `FileSystemResolver` and `PackageResolver`, that is enabled by default.
- `PackageResolver` has now the cache as generic type argument.
- `PackageResolver` has to be build with the `PackageResolverBuilder`

## [0.9.0] - 2024-10-12

- Fix: Today function - Use Utc::now instead of Local::now
- Support packages, that are installed locally. ([local typst package dir](https://github.com/typst/packages?tab=readme-ov-file#local-packages))

- Breaking: Support caching packages in file system (default: <OS_CACHE_DIR>/typst/packages). Library users now have to specify, if they want to use in memory caching or the file system. Default is file system.

Change

```rust
    let arc = Default::default();
    let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
        .with_package_file_resolver(arc, None);
```

to

```rust
    let arc = Default::default();
    let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
        .add_file_resolver(PackageResolver::new(PackageResolverCache::Memory(arc), None));
```

You also can use the filesystem now, which is the default:

```rust
    let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
        .with_package_file_resolver(None);
```
