use derive_typst_intoval::{IntoDict, IntoValue};
use std::fs;
use typst::foundations::{Bytes, Dict, IntoValue};
use typst_as_lib::TypstEngine;

static TEMPLATE_FILE: &str = include_str!("./templates/html.typ");
static OUTPUT: &str = "./examples/output.html";
static IMAGE: &[u8] = include_bytes!("./templates/images/typst.png");

fn main() {
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstEngine::builder().main_file(TEMPLATE_FILE).build();

    // Run it
    let doc = template
        .compile_with_input(dummy_data())
        .output
        .expect("typst::compile() returned an error!");

    // Create html
    let html = typst_html::html(&doc).expect("Could not generate HTML.");
    fs::write(OUTPUT, html).expect("Could not write HTML.");
}

// Some dummy content. We use `derive_typst_intoval` to easily
// create `Dict`s from structs by deriving `IntoDict`;
fn dummy_data() -> Content {
    Content {
        v: vec![
            ContentElement {
                heading: "Foo".to_owned(),
                text: Some("Hello World!".to_owned()),
                num1: 1,
                num2: Some(42),
                image: Some(Bytes::new(IMAGE.to_vec())),
            },
            ContentElement {
                heading: "Bar".to_owned(),
                num1: 2,
                ..Default::default()
            },
        ],
    }
}

#[derive(Debug, Clone, IntoValue, IntoDict)]
struct Content {
    v: Vec<ContentElement>,
}

// Implement Into<Dict> manually, so we can just pass the struct
// to the compile function.
impl From<Content> for Dict {
    fn from(value: Content) -> Self {
        value.into_dict()
    }
}

#[derive(Debug, Clone, Default, IntoValue, IntoDict)]
struct ContentElement {
    heading: String,
    text: Option<String>,
    num1: i32,
    num2: Option<i32>,
    image: Option<Bytes>,
}
