# Typst as lib

Small wrapper around [Typst](https://github.com/typst/typst) that makes it easier to use it as a templating engine.
This API is currently not really stable, although I try to implement the features with as little change to the API as possible.

## Usage

#### rust code

```rust
// main.rs
static TEMPLATE_FILE: &str = include_str!("./templates/template.typ");
static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
static OUTPUT: &str = "./examples/output.pdf";
static IMAGE: &[u8] = include_bytes!("./templates/images/typst.png");

fn main() {
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstEngine::builder()
        .main_file(TEMPLATE_FILE)
        .fonts([FONT])
        .build();

    // Run it
    let doc = template
        .compile_with_input(dummy_data())
        .output
        .expect("typst::compile() returned an error!");

    // Create pdf
    let options = Default::default();
    let pdf = typst_pdf::pdf(&doc, &options)
        .expect("Could not generate pdf.");
    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}
```

[Full example file](https://github.com/Relacibo/typst-as-lib/blob/main/examples/small_example.rs)

#### Typst code

```typ
// template.typ
#import sys: inputs

#set page(paper: "a4")
#set text(font: "TeX Gyre Cursor", 11pt)

#let content = inputs.v
#let last_index = content.len() - 1

#for (i, elem) in content.enumerate() [
  == #elem.heading
  Text: #elem.text \
  Num1: #elem.num1 \
  Num2: #elem.num2 \
  #if elem.image != none [#image.decode(elem.image, height: 40pt)]
  #if i < last_index [
    #pagebreak()
  ]
]
```

Run example with:

```bash
cargo r --example=small_example
```

## Resolving files

### Binaries and Sources

Use `TypstEngineBuilder::with_static_file_resolver` and add the binaries as key value pairs (`(file_name, &[u8])`) to add static binary files, that typst can use.

Use `TypstEngineBuilder::with_static_source_file_resolver` and add the sources as key value pairs (`(file_name, String)`) to add static `Source`s.

See example [resolve_static](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_static.rs) which uses the static file resolvers.

```bash
cargo r --example=resolve_static
```

### Local files

Resolving local files can be enabled with `TypstEngineBuilder::with_file_system_resolver`. The root should be the template folder. Files cannot be resolved, if they are outside of root.

Can be enabled like this:

```rust
let template = TypstEngine::builder()
    .main_file(TEMPLATE_FILE)
    .fonts([font])
    .with_file_system_resolver("./examples/templates")
    .build();
```

If you want to use another local package install path, use:

```rust
let template = TypstEngine::builder()
    .main_file(TEMPLATE_FILE)
    .fonts([font])
    .add_file_resolver(
        FileSystemResolver::new("./examples/templates")
            .local_package_root("local/packages")
            .into_cached()
    )
    .build();
```

### Remote packages

The features `package` and one of `ureq` or `reqwest` need to be enabled.

Can be enabled like this:

```rust
let template = TypstEngine::builder()
    .main_file(TEMPLATE_FILE)
    .fonts([font])
    .with_package_file_resolver()
    .build();
```

This uses the file system as a cache.

If you want to use another cache root path, use:

```rust
let template = TypstEngine::builder()
    .main_file(TEMPLATE_FILE)
    .fonts([font])
    .add_file_resolver(PackageResolver::new()
        .cache(
            FileSystemCache(PathBuf::from("cache/root"))
        )
        .build()
        .into_cached()
    )
    .build();
```
Note that the Cache Wrapper created with the call to `into_cached(self)` creates a in memory cache for Binary files and `Source` files. 

If you want to instead use the memory as (binary) cache, use:

```rust
let template = TypstEngine::build()
    .main_file(TEMPLATE_FILE)
    .add_fonts([font])
    .add_file_resolver(PackageResolver::builder()
        .cache(
            InMemoryCache::new()
        )
        .build()
        .into_cached()
    )
    .build();
```

Note that the Cache Wrapper created with the call to `into_cached(self)` only caches the `Source` files (each single file lazily) here and the `InMemoryCache` caches all binaries (eagerly after the first download of the whole package, which is triggered (lazily), when requested in a typst script).

### Local files and remote packages example

See example [resolve_packages](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_packages.rs) which uses the file and the package resolver.

```bash
cargo r --example=resolve_files --features=package
```

### Custom file resolver

You can also write your own file resolver. You need to implement the Trait `FileResolver` and pass it to the `TypstEngineBuilder::add_file_resolver` function. There you can also have some additional config options for the `PackageFileResolver`, `FileSystemResolver`, and so on.

## Loading fonts

You can simply add fonts to the `TypstEngine` with `TypstEngineBuilder::fonts`. You can also activate the feature `typst-kit-fonts` that adds `search_fonts_with` to `TypstEngineBuilder`, which uses the `typst-kit` library to resolve system fonts. You also might additionally use the feature `typst-kit-embed-fonts`, that activates the feature `embed-fonts` for `typst-kit`. This causes `typst-kit` to also embed fonts from [typst-assets](https://github.com/typst/typst-assets) at compile time.

See example [font_searcher](https://github.com/Relacibo/typst-as-lib/blob/main/examples/font_searcher.rs).

```bash
cargo r --example=font_searcher --features=typst-kit-fonts,typst-kit-embed-fonts
```

### main file

The `TypstEngine::main_file` call is not needed, it's just for conveniance. You can omit it, and then you pass it to the `TypstEngine::compile` call later. (See example [resolve_static](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_static.rs))

## TODO
- Maybe `packages` WASM support, if possible... 
- Make "static `Source`s/binary files" added with `TypstEngineBuilder::with_static_[file/source_file]_resolver` and main file editable inbetween `compile` calls
- Support multiple typst versions with feature flags

## Previous work

- [https://github.com/tfachmann/typst-as-library](https://github.com/tfachmann/typst-as-library)

## Maybe useful

- [https://github.com/KillTheMule/derive_typst_intoval](https://github.com/KillTheMule/derive_typst_intoval)
