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
    let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");

    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstTemplate::new(vec![font], TEMPLATE_FILE);

    let mut tracer = Tracer::new();

    // Run it
    // Run `template.compile(&mut tracer)` to run typst script
    // without any input.
    let doc = template
        .compile_with_input(&mut tracer, dummy_data())
        .expect("typst::compile() returned an error!");

    // Create pdf
    let pdf = typst_pdf::pdf(&doc, Smart::Auto, None);
    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}
```

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

[Full example file](https://github.com/Relacibo/typst-as-lib/blob/main/examples/small_example.rs)

Run example with:

```bash
cargo r --example=small_example
```

### TypstTemplateCollection

If you want to compile multiple typst (main) source files you might want to use the `TypstTemplateCollection`, which allows you to specify the source file, when calling `TypstTemplateCollection::compile`, instead of passing it to new. The source file has to be added with `TypstTemplateCollection::add_static_file_resolver` first.
`TypstTemplate` is just a wrapper around `TypstTemplateCollection`, that also saves a `FileId` for the main source file.

### Resolving files in memory
Use `TypstTemplate::with_static_file_resolver` and add the sources and binaries as key value pairs (`(file_name, Source/&[u8])`)

### Resolving files and packages

Resolving local files can be enabled with `TypstTemplate::with_file_system_resolver`. Resolving packages can be enabled with `TypstTemplate::with_package_file_resolver`.

See [example](https://github.com/Relacibo/typst-as-lib/blob/main/examples/resolve_packages.rs) which uses the file and the package resolver. The `package` feature needs to be enabled.

```bash
cargo r --example=resolve_files --features=package
```

## Loading fonts

Loading fonts is not in the scope of this library (yet?). If you are interested in that, write an issue.

- [This](https://github.com/typst/typst/blob/a2c980715958bc3fd71e1f0a5975fea3f5b63b85/crates/typst-cli/src/fonts.rs#L69) is how the typst-cli loads system fonts.
- Here is an [example](https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L216) of loading fonts from a folder.

## Some links, idk

- [https://github.com/tfachmann/typst-as-library](https://github.com/tfachmann/typst-as-library)
- [https://github.com/KillTheMule/derive_typst_intoval](https://github.com/KillTheMule/derive_typst_intoval)
