use derive_typst_intoval::{IntoDict, IntoValue};
use std::fs;
use typst::foundations::{Bytes, Dict, IntoValue, Smart};
use typst::text::Font;
use typst_as_lib::TypstTemplate;

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

    // Run it
    let doc = template
        .compile_with_input(dummy_data())
        .output
        .expect("typst::compile() returned an error!");

    // Create pdf
    let options = Default::default();
    let pdf = typst_pdf::pdf(&doc, &options).expect("Could not generate pdf.");
    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
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
                image: Some(Bytes::from(IMAGE)),
            },
            ContentElement {
                heading: "Bar".to_owned(),
                num1: 2,
                ..Default::default()
            },
        ],
    }
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
    image: Option<Bytes>,
}
