use std::fs;
use typst::foundations::{Dict, IntoValue};
use typst_as_lib::TypstEngine;
use std::collections::HashMap;

static TEMPLATE_FILE: &str = include_str!("./templates/template_hashmap.typ");
static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
static OUTPUT: &str = "./examples/output.pdf";

fn main() {
    // Read in fonts and the main source file.
    // We can use this template more than once, if needed (Possibly
    // with different input each time).
    let template = TypstEngine::builder()
        .main_file(TEMPLATE_FILE)
        .fonts([FONT])
        .build();

    // Run it
    let doc = template
        .compile_with_input(dummy_data())
        .output
        .expect("typst::compile() returned an error!");

    // Create pdf
    let options = Default::default();
    let pdf = typst_pdf::pdf(&doc, &options).expect("Could not generate pdf.");
    fs::write(OUTPUT, pdf).expect("Could not write pdf.");
}

// Some dummy content. We use `derive_typst_intoval` to easily
// create `Dict`s from structs by deriving `IntoDict`;
fn dummy_data() -> Content {
    let mut hashmap = HashMap::new();
    let mut hashmap_vec = HashMap::new();
    // Key must not contains blank character
    hashmap.insert(String::from("key1"), String::from("value1"));
    hashmap.insert(String::from("key2"), String::from("value2"));
    hashmap_vec.insert(String::from("key3"), vec![1, 2, 3]);
    Content {
        values: hashmap,
        values_vec: hashmap_vec, 
    }
}

#[derive(Debug, Clone)]
struct Content {
    values: HashMap<String, String>,
    values_vec: HashMap<String, Vec<i32>>,
}

// Implement Into<Dict> manually, so we can just pass the hashmap in the struct
// to the compile function.
impl From<Content> for Dict {
    fn from(value: Content) -> Self {
        let mut dict = Dict::new();
        for (key, value) in value.values {
            dict.insert(key.into(), value.into_value());
        }
        for (key, value) in value.values_vec {
            dict.insert(key.into(), value.into_value());
        }
        dict
    }
}
