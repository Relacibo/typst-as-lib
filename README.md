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
    let font = Font::new(Bytes::from(FONT), 0)
        .expect("Could not parse font!");

    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstTemplate::new(vec![font], TEMPLATE_FILE);

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
#import typst_as_lib: input

#set page(paper: "a4")
#set text(font: "TeX Gyre Cursor", 11pt)

#let content = input.v
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
### Binaries
Use `TypstTemplate::with_static_file_resolver` and add the binaries as key value pairs (`(file_name, &[u8])`).

### Sources
Use `TypstTemplate::with_static_source_file_resolver` and add the sources as key value pairs (`(file_name, String)`).



### Local files
Resolving local files can be enabled with `TypstTemplate::with_file_system_resolver`. The root should be the template folder. Files cannot be resolved, if they are outside of root.

Can be enabled like this:
```rust
let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
    .with_file_system_resolver("./examples/templates");
```

If you want to use another local package install path, use:
```rust
let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
    .add_file_resolver(
        FileSystemResolver::new("./examples/templates")
            .with_local_package_root("local/packages")
            .cached()
    );
```

### Remote Packages
The `package` feature needs to be enabled.

Can be enabled like this:
```rust
let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
    .with_package_file_resolver(None);
```

This uses the file system as a cache. 

If you want to use another cache root path, use:
```rust
let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
    .add_file_resolver(PackageResolverBuilder::new()
        .set_cache(
            FileSystemCache(PathBuf::from("cache/root"))
        )
        .build().cached()
    );
```

If you want to instead use the memory as cache, use:
```rust
let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
    .add_file_resolver(PackageResolverBuilder::new()
        .set_cache(
            InMemoryCache::new()
        )
        .build().cached()
    );
```

### Examples

#### Static binaries and sources

See [example](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_static.rs) which uses the static file resolvers.

```bash
cargo r --example=resolve_static
```

#### Local files and remote packages

See [example](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_packages.rs) which uses the file and the package resolver. 

```bash
cargo r --example=resolve_files --features=package
```

### Custom file resolver

You can also write your own file resolver. You need to implement the Trait `FileResolver` and  pass it to the `TypstTemplate::add_file_resolver` function.

## TypstTemplateCollection

If you want to compile multiple typst (main) source files you might want to use the `TypstTemplateCollection`, which allows you to specify the source file, when calling `TypstTemplateCollection::compile`, instead of passing it to new. The source file has to be added with `TypstTemplateCollection::add_static_file_resolver` first.
`TypstTemplate` is just a wrapper around `TypstTemplateCollection`, that also saves a `FileId` for the main source file.

## Loading fonts

Loading fonts is not in the scope of this library (yet?). If you are interested in that, write an issue.

- [This](https://github.com/typst/typst/blob/a2c980715958bc3fd71e1f0a5975fea3f5b63b85/crates/typst-cli/src/fonts.rs#L69) is how the typst-cli loads system fonts.
- Here is an [example](https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L216) of loading fonts from a folder.

## TODO
- allow usage of reqwest instead of ureq with a feature flag
- fonts

## Some links, idk

- [https://github.com/tfachmann/typst-as-library](https://github.com/tfachmann/typst-as-library)
- [https://github.com/KillTheMule/derive_typst_intoval](https://github.com/KillTheMule/derive_typst_intoval)
