use derive_typst_intoval::{IntoDict, IntoValue};
use std::fs;
use typst::foundations::{Bytes, IntoValue, Smart};
use typst::text::Font;
use typst_as_lib::TypstTemplate;

static TEMPLATE_FILE: &str = include_str!("./templates/template.typ");

static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

fn main() {
    let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");
    let template = TypstTemplate::new(TEMPLATE_FILE.to_owned()).with_fonts(vec![font]);

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

    let doc = template
        .compile(&mut tracer, content.into_dict())
        .expect("Something went wrong!");

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
