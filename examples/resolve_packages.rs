#[cfg(feature = "packages")]
use std::fs;
#[cfg(feature = "packages")]
use typst::foundations::Bytes;
#[cfg(feature = "packages")]
use typst::text::Font;
#[cfg(feature = "packages")]
use typst_as_lib::TypstEngine;

#[cfg(feature = "packages")]
static OUTPUT: &str = "./examples/output.pdf";

#[cfg(feature = "packages")]
static TEMPLATE_FILE: &str = include_str!("./templates/resolve_files.typ");

#[cfg(feature = "packages")]
static ROOT: &str = "./examples/templates";

#[cfg(feature = "packages")]
static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

#[cfg(feature = "packages")]
fn main() {

    let font = Font::new(Bytes::new(FONT.to_vec()), 0).expect("Could not parse font!");

    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstEngine::builder()
        .main_file(TEMPLATE_FILE)
        .fonts([font])
        .with_file_system_resolver(ROOT)
        .with_package_file_resolver()
        .build();

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

#[cfg(not(feature = "packages"))]
fn main() {
    eprintln!("You need to run this with flag `--features=packages`!")
}
