//! Automatic package bundling demo
//!
//! How to run:
//! 1. export TYPST_TEMPLATE_DIR=./examples/templates
//! 2. cargo build --example auto_packages --features package-bundling,ureq
//! 3. cargo run --example auto_packages --features package-bundling,ureq

#[cfg(feature = "package-bundling")]
use std::fs;

#[cfg(feature = "package-bundling")]
use typst_as_lib::TypstEngine;

static TEMPLATE: &str = include_str!("./templates/with_packages.typ");
static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
static OUTPUT: &str = "./examples/output_auto.pdf";

#[cfg(feature = "package-bundling")]
fn main() {
    println!("Building engine with bundled packages...");

    let engine = TypstEngine::builder()
        .main_file(TEMPLATE)
        .fonts([FONT])
        .with_bundled_packages() // Use embedded packages
        .build();

    println!("Compiling document...");

    let doc = engine.compile().output.expect("Compilation failed");

    println!("Generating PDF...");

    let pdf = typst_pdf::pdf(&doc, &Default::default()).expect("PDF generation failed");

    fs::write(OUTPUT, pdf).expect("Write failed");

    println!("✓ PDF generated: {}", OUTPUT);
    println!("  Packages served from embedded data (no filesystem access!)");
}

#[cfg(not(feature = "package-bundling"))]
fn main() {
    eprintln!("This example requires 'package-bundling' feature");
    eprintln!("Run with:");
    eprintln!("  export TYPST_TEMPLATE_DIR=./examples/templates");
    eprintln!("  cargo run --example auto_packages --features package-bundling,ureq");
}
