#[cfg(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts"))]
static TEMPLATE_FILE: &str = include_str!("./templates/font_searcher.typ");

#[cfg(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts"))]
static OUTPUT: &str = "./examples/output.pdf";

#[cfg(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts"))]
fn main() {
    use typst_as_lib::{typst_kit_options, TypstEngine};
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).

    let template = TypstEngine::builder()
        .main_file(TEMPLATE_FILE)
        .search_fonts_with(
            typst_kit_options::TypstKitFontOptions::default()
                .include_system_fonts(false)
                // This line is not necessary, because thats the default.
                .include_embedded_fonts(true),
        )
        .build();

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

#[cfg(not(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts")))]
fn main() {
    eprintln!("You need to run this with flag `--features=typst-kit-fonts,typst-kit-embed-fonts`!")
}
