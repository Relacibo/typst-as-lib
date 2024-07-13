# typst-as-lib

Small wrapper for typst that allows using it as a templating engine. Maybe useful for someone...

## Usage

### rust code

```rust
// main.rs
use derive_typst_intoval::{IntoDict, IntoValue};
use std::fs;
use typst::foundations::{Bytes, IntoValue, Smart};
use typst::text::Font;
use typst_as_lib::TypstTemplate;

static TEMPLATE_FILE: &str = include_str!(
    "./templates/template.typ"
);

static FONT: &[u8] = include_bytes!(
    "./fonts/texgyrecursor-regular.otf"
);

fn main() {
    let font = Font::new(Bytes::from(FONT), 0)
                  .expect("Could not parse font!");

    // Read in fonts and the main source file. 
    // It will be assigned the id "/template.typ".
    // We can use this template more than once, 
    // if needed (Possibly with different input each time).
    let template = TypstTemplate::new(
        vec![font], TEMPLATE_FILE.to_owned()
    );

    // optionally pass in some additional source files.
    // `other_sources` is of type `HashMap<FileId, String>`.
    // template = template.with_other_sources(other_sources);

    // optionally pass in some additional binary files.
    // `files` is of type `HashMap<FileId, &[u8]>`.
    // template = template.with_binary_files(files);

    // Some dummy content. We use `derive_typst_intoval` to 
    // easily create `Dict`s from structs by deriving `IntoDict`;
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
        .compile_with_input(&mut tracer, content.into_dict())
        .expect("typst::compile() returned an error!");

    // Create pdf
    let pdf = typst_pdf::pdf(&doc, Smart::Auto, None);
    fs::write("./output.pdf", pdf).expect("Could not write pdf.");
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

### typst code

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

## Some links, idk

- [https://github.com/tfachmann/typst-as-library](https://github.com/tfachmann/typst-as-library)
- [https://github.com/KillTheMule/derive_typst_intoval](https://github.com/KillTheMule/derive_typst_intoval)
