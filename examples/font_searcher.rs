use typst_as_lib::{typst_kit_options::TypstKitFontOptions, TypstEngine};
static TEMPLATE_FILE: &str = include_str!("./templates/font_searcher.typ");
static OUTPUT: &str = "./examples/output.pdf";

fn main() {
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).

    let template = TypstEngine::builder()
        .main_file(TEMPLATE_FILE)
        .search_fonts_with(
            TypstKitFontOptions::default()
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
