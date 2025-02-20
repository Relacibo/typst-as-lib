#[cfg(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts"))]
static TEMPLATE_FILE: &str = include_str!("./templates/font_searcher.typ");

#[cfg(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts"))]
use typst_as_lib::TypstTemplate;

#[cfg(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts"))]
static OUTPUT: &str = "./examples/output.pdf";

#[cfg(all(feature = "typst-kit-fonts", feature = "typst-kit-embed-fonts"))]
fn main() {
    use typst_as_lib::font_searcher_options::FontSearcherOptions;
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).

    let template = TypstTemplate::new(TEMPLATE_FILE).search_fonts_with(
        FontSearcherOptions::default()
            .include_system_fonts(false)
            // This line is not necessary, because thats the default.
            .include_embedded_fonts(true),
    );

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
