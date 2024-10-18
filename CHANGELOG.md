# Changelog

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
