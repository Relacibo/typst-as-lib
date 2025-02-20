#[cfg(feature = "typst-kit-fonts")]
static TEMPLATE_FILE: &str = include_str!("./templates/font_searcher.typ");

#[cfg(feature = "typst-kit-fonts")]
use typst_as_lib::TypstTemplate;

#[cfg(feature = "typst-kit-fonts")]
static OUTPUT: &str = "./examples/output.pdf";

#[cfg(feature = "typst-kit-fonts")]
fn main() {
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).

    let template = TypstTemplate::new(TEMPLATE_FILE).search_fonts_with(Default::default());

    // Run it
    let doc = template
        .compile()
        .output
        .expect("typst::compile() returned an error!");

    let options = Default::default();

    // Create pdf
    let pdf = typst_pdf::pdf(&doc, &options).expect("Could not generate pdf.");
    std::fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}

#[cfg(not(feature = "typst-kit-fonts"))]
fn main() {
    eprintln!("You need to run this with flag `--features=typst-kit-fonts`!")
}
