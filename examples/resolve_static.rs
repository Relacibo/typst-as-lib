use std::fs;
use typst::foundations::{Bytes, Smart};
use typst::text::Font;
use typst_as_lib::TypstTemplate;

static TEMPLATE_FILE: &str = include_str!("./templates/resolve_static.typ");

static OTHER_TEMPLATE_FILE: &str = include_str!("./templates/function.typ");

static IMAGE: &[u8] = include_bytes!("./templates/images/typst.png");

static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

static OUTPUT: &str = "./examples/output.pdf";

fn main() {
    let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");

    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
        .with_static_source_file_resolver([("function.typ", OTHER_TEMPLATE_FILE)])
        .with_static_file_resolver([("./images/typst.png", IMAGE)]);

    // Run it
    let doc = template
        .compile()
        .output
        .expect("typst::compile() returned an error!");

    let options = Default::default();

    // Create pdf
    let pdf = typst_pdf::pdf(&doc, &options).expect("Could not generate pdf.");

    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}
