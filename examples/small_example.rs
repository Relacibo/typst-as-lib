use std::fs;
use typst::foundations::{Bytes, Dict, IntoValue, Value};
use typst_as_lib::TypstEngine;

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
    let pdf = typst_pdf::pdf(&doc, &options).expect("Could not generate pdf.");
    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}

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

#[derive(Debug, Clone)]
struct Content {
    v: Vec<ContentElement>,
}

impl From<Content> for Dict {
    fn from(value: Content) -> Self {
        let mut dict = Dict::new();
        dict.insert("v".into(), value.v.into_value());
        dict
    }
}

impl IntoValue for Content {
    fn into_value(self) -> Value {
        Value::Dict(self.into())
    }
}

#[derive(Debug, Clone, Default)]
struct ContentElement {
    heading: String,
    text: Option<String>,
    num1: i32,
    num2: Option<i32>,
    image: Option<Bytes>,
}

impl IntoValue for ContentElement {
    fn into_value(self) -> Value {
        let mut dict = Dict::new();
        dict.insert("heading".into(), self.heading.into_value());
        dict.insert("text".into(), self.text.into_value());
        dict.insert("num1".into(), self.num1.into_value());
        dict.insert("num2".into(), self.num2.into_value());
        dict.insert("image".into(), self.image.into_value());
        Value::Dict(dict)
    }
}
