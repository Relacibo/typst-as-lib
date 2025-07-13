static OUTPUT: &str = "./examples/output.pdf";
static TEMPLATE_FILE: &str = include_str!("./templates/resolve_files.typ");
static ROOT: &str = "./examples/templates";
static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");

#[cfg(any(feature = "async-reqwest"))]
fn main() {
}

#[cfg(not(any(feature = "async-reqwest")))]
fn main() {
    eprintln!(r#"Enable the async-reqwest feature"#)
}
