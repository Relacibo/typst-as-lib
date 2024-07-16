# typst-as-lib

Small wrapper for typst that makes it easier to use it as a templating engine. Maybe useful for someone...

## Usage

### TypstTemplate

#### rust code

```rust
// main.rs
use derive_typst_intoval::{IntoDict, IntoValue};
use std::fs;
use typst::foundations::{Bytes, Dict, IntoValue, Smart};
use typst::text::Font;
use typst_as_lib::TypstTemplate;

static TEMPLATE_FILE: &str = include_str!("./templates/template.typ");

static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

// static OTHER_SOURCE: &str = include_str!("./templates/other_source.typ");

// static IMAGE: &[u8] = include_bytes!("./images/image.png");

fn main() {
    let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");

    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly 
    // with different input each time).
    #[allow(unused_mut)]
    let mut template = TypstTemplate::new(vec![font], TEMPLATE_FILE);

    // optionally set a custom inject location, which will have a
    // better performance, when reusing the template
    // template = template.custom_inject_location("from_rust", "inputs");

    // optionally pass in some additional source files.
    // let source = ("/other_source.typ", OTHER_SOURCE);    
    // template = template.add_other_sources([source]);

    // optionally pass in some additional binary files.
    // let tuple = ("/images/image.png", IMAGE);
    // template = template.add_binary_files([tuple]);

    // Some dummy content. We use `derive_typst_intoval` to easily
    // create `Dict`s from structs by deriving `IntoDict`;
    let content = Content {
        v: vec![
            ContentElement {
                heading: "Heading".to_owned(),
                text: Some("Text".to_owned()),
                num1: 1,
                num2: Some(2),
            },
            ContentElement {
                heading: "Heading2".to_owned(),
                num1: 2,
                ..Default::default()
            },
        ],
    };

    let mut tracer = Default::default();

    // Run it
    // Run `template.compile(&mut tracer)` to run typst script
    // without any input.
    let doc = template
        .compile_with_input(&mut tracer, content)
        .expect("typst::compile() returned an error!");

    // Create pdf
    let pdf = typst_pdf::pdf(&doc, Smart::Auto, None);
    fs::write("./output.pdf", pdf).expect("Could not write pdf.");
}

// Implement Into<Dict> manually, so we can just pass the struct
// to the compile function.
impl From<Content> for Dict {
    fn from(value: Content) -> Self {
        value.into_dict()
    }
}

#[derive(Debug, Clone, IntoValue, IntoDict)]
struct Content {
    v: Vec<ContentElement>,
}

#[derive(Debug, Clone, Default, IntoValue, IntoDict)]
struct ContentElement {
    heading: String,
    text: Option<String>,
    num1: i32,
    num2: Option<i32>,
}
```

#### typst code

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
  #if i < last_index [
    #pagebreak()
  ]
]
```

Run example with `cargo r --example small_example`.

### TypstTemplateCollection

If you want to compile multiple typst source files you might want to use the `TypstTemplateCollection`, which allows you to specify the source file, when calling `TypstTemplateCollection::compile`, instead of passing it to new. The source file has to be added with `TypstTemplateCollection::add_sources` first.

## Loading fonts

Loading fonts is not in the scope of this library (yet). 
- This is how the typst-cli loads system fonts:

    [https://github.com/typst/typst/blob/a2c980715958bc3fd71e1f0a5975fea3f5b63b85/crates/typst-cli/src/fonts.rs#L69](https://github.com/typst/typst/blob/a2c980715958bc3fd71e1f0a5975fea3f5b63b85/crates/typst-cli/src/fonts.rs#L69)
- Here is an example of loading fonts from a folder:
  
    [https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L216](https://github.com/tfachmann/typst-as-library/blob/dd9a93379b486dc0a2916b956360db84b496822e/src/lib.rs#L216)

## Some links, idk

- [https://github.com/tfachmann/typst-as-library](https://github.com/tfachmann/typst-as-library)
- [https://github.com/KillTheMule/derive_typst_intoval](https://github.com/KillTheMule/derive_typst_intoval)
