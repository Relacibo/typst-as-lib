use std::fs;
use typst::foundations::{Bytes, Smart};
use typst::text::Font;
use typst_as_lib::TypstTemplate;

static TEMPLATE_FILE: &str = include_str!("./templates/resolve_files.typ");

static ROOT: &str = "./examples/templates";

static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

static OUTPUT: &str = "./examples/output.pdf";

#[cfg(feature = "packages")]
fn main() {
    let font = Font::new(Bytes::from(FONT), 0).expect("Could not parse font!");

    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstTemplate::new(vec![font], TEMPLATE_FILE)
        .with_file_system_resolver(ROOT)
        .with_package_file_resolver(None);
    let mut tracer = Default::default();

    // Run it
    let doc = template
        .compile(&mut tracer)
        .expect("typst::compile() returned an error!");

    // Create pdf
    let pdf = typst_pdf::pdf(&doc, Smart::Auto, None);
    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}

#[cfg(not(feature = "packages"))]
fn main() {
    eprintln!("You need to run this with flag `--features=packages`!")
}
