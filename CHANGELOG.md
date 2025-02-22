# Changelog

## [0.14.0] - 2025-02-22

- Added package resolving using `reqwest` (blocking) instead of `ureq`
- Added features `ureq` and `reqwest`. When using feature `packages`, the user has to choose between one of those for use as http library.
- Added function `request_retry_count` in `PackageFileResolverBuilder`, so the user can optionally set the count of maximum request retries (Default is 3).
- Deprecated `PackageResolver::new()` in favour of `PackageResolver::builder()`.
- Renamed function `PackageFileResolverBuilder::set_cache` to `PackageFileResolverBuilder::cache` to be more consistent.

## [0.13.0] - 2025-02-22

- `TypstTemplate` and `TypstTemplateCollection` have been more or less replaced by `TypstEngine`.
- `TypstEngine::builder()` gives you the `TypstEngineBuilder`, where the Engine can be configured.
- `TypstEngineBuilder::build()` then builds the `TypstEngine`.
- `TypstEngine::compile` and `TypstEngine::compile_with_input` remain more or less the same.
- All `*_mut` functions have been scrapped.
- The `TypstTemplateCollection::add_fonts` has been moved/renamed to `TypstEngine::fonts`.
- `TypstEngine::fonts` now accepts `&[u8]` and `Vec<u8>` and reads out the font automatically.
- Sorry for messing with the API so much. Im just never content...

### Migration

- instead of `TypstTemplate[Collection]::new()` use: `TypstEngine::builder()` and after configuration call `TypstEngineBuilder::build()`.
- instead of `TypstTemplate::new(file)` use `TypstEngine::builder().main_file(file). ... .build()`

## [0.12.2] - 2025-02-20

- `FontSearcherOptions` variables are now `pub(crate)`
- Add `typst-kit-embed-fonts` feature to use `typst-kit` feature `embed-fonts`. This causes `typst-kit` use fonts from [typst-assets](https://github.com/typst/typst-assets).

## [0.12.1] - 2025-02-20

- Add getter for fonts variable in `TypstTemplateCollection`, with feature `typst-kit-fonts`.
- Removed unnecessary (each font slot has a OnceLock) Arc<Mutex<\_>> wrapper around fonts with feature `typst-kit-fonts`.
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
