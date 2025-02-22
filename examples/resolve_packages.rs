#[cfg(any(feature = "ureq", feature = "reqwest"))]
use std::fs;

#[cfg(any(feature = "ureq", feature = "reqwest"))]
use typst_as_lib::TypstEngine;

static OUTPUT: &str = "./examples/output.pdf";
static TEMPLATE_FILE: &str = include_str!("./templates/resolve_files.typ");
static ROOT: &str = "./examples/templates";
static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

#[cfg(any(feature = "ureq", feature = "reqwest"))]
fn main() {
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstEngine::builder()
        .main_file(TEMPLATE_FILE)
        .fonts([FONT])
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

#[cfg(not(any(feature = "ureq", feature = "reqwest")))]
fn main() {
    eprintln!(r#"Feature "ureq" or "reqwest" needs to be enabled"#)
}
