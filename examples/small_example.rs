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
