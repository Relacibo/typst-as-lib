//! Fetches `@preview` packages ahead of compilation and feeds them into the
//! engine as static resolvers.
//!
//! Typst's `World` only does blocking IO, which browser WASM can't provide
//! mid-compile — so hosts fetch packages ahead of the compile, host-side.
//! How to fetch is the host's choice; this example uses blocking `ureq`.
//!
//! Run: `cargo run --features packages,ureq --example prefetch_packages`

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use binstall_tar::Archive;
use typst::syntax::{
    FileId, RootedPath, SyntaxNode, VirtualPath, VirtualRoot, ast, package::PackageSpec,
};
use typst_as_lib::TypstEngine;
use typst_layout::PagedDocument;

static OUTPUT: &str = "./examples/output.pdf";
static FONT: &[u8] = include_bytes!("./fonts/texgyrecursor-regular.otf");
static TEMPLATE: &str = r#"#import "@preview/theorion:0.3.0": *
#import cosmos.fancy: *
#show: show-theorion

#theorem(title: "Prefetched")[
  This theorem box was rendered from a package fetched ahead of compilation —
  and its dependency (showybox) was discovered transitively by the scan.
]
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1) Scan: find package imports in the template, transitively (theorion
    //    pulls in showybox — packages import packages). Malformed imports are
    //    skipped; a host must never panic on template-controlled input.
    let mut found: HashSet<PackageSpec> = HashSet::new();
    let mut queue: Vec<PackageSpec> = Vec::new();
    scan_packages(TEMPLATE, &mut found, &mut queue);

    // 2) Fetch each package's tarball.
    let mut fetched: HashSet<PackageSpec> = HashSet::new();
    let mut package_sources: Vec<(FileId, String)> = Vec::new();
    let mut package_binaries: Vec<(FileId, Vec<u8>)> = Vec::new();
    while let Some(spec) = queue.pop() {
        if !fetched.insert(spec.clone()) {
            continue;
        }
        let tarball = fetch_package(&spec)?;

        // 3) Extract — and scan the extracted sources: a template's packages
        //    may pull in further packages.
        for (path, bytes) in extract(&tarball)? {
            let Ok(vpath) = VirtualPath::new(path.to_string_lossy().as_ref()) else {
                continue;
            };
            let id = RootedPath::new(VirtualRoot::Package(spec.clone()), vpath).intern();
            if path.extension().is_some_and(|ext| ext == "typ") {
                let Ok(text) = String::from_utf8(bytes) else {
                    continue;
                };
                scan_packages(&text, &mut found, &mut queue);
                package_sources.push((id, text));
            } else {
                package_binaries.push((id, bytes));
            }
        }
    }

    // 4) Feed: package resolvers first (resolver order = try order), then the
    //    template file. The engine compiles without any blocking IO.
    let engine = TypstEngine::builder()
        .with_static_source_file_resolver(package_sources)
        .with_static_file_resolver(package_binaries)
        .with_static_source_file_resolver([("main.typ", TEMPLATE)])
        .fonts([FONT])
        .build();

    let doc: PagedDocument = engine.compile("main.typ").output?;

    let pdf = typst_pdf::pdf(&doc, &Default::default()).expect("Could not generate pdf.");
    fs::write(OUTPUT, pdf)?;
    println!("wrote {OUTPUT}");
    Ok(())
}

/// Collects package imports in `text` — including `#include` — transitively
/// into `queue`. Malformed imports are skipped: a host must never panic on
/// template-controlled input.
fn scan_packages(text: &str, found: &mut HashSet<PackageSpec>, queue: &mut Vec<PackageSpec>) {
    walk_imports(&typst::syntax::parse(text), found, queue);
}

fn walk_imports(node: &SyntaxNode, found: &mut HashSet<PackageSpec>, queue: &mut Vec<PackageSpec>) {
    if node.cast::<ast::ModuleImport>().is_some() || node.cast::<ast::ModuleInclude>().is_some() {
        for child in node.children() {
            let Some(str_node) = child.cast::<ast::Str>() else {
                continue;
            };
            let import = str_node.get();
            if !import.starts_with('@') {
                continue;
            }
            let Ok(spec) = import.parse::<PackageSpec>() else {
                continue;
            };
            if found.insert(spec.clone()) {
                queue.push(spec);
            }
        }
        return;
    }
    for child in node.children() {
        walk_imports(child, found, queue);
    }
}

/// Only the `preview` namespace is hosted on the package CDN; `@local`
/// packages are the host's responsibility.
fn fetch_package(spec: &PackageSpec) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if spec.namespace != "preview" {
        return Err(format!("cannot fetch package namespace '{}'", spec.namespace).into());
    }
    let url = format!(
        "https://packages.typst.org/preview/{}-{}.tar.gz",
        spec.name, spec.version
    );
    println!("fetching {url}");
    Ok(ureq::get(&url).call()?.into_body().read_to_vec()?)
}

type TarEntry = (PathBuf, Vec<u8>);

fn extract(tarball: &[u8]) -> Result<Vec<TarEntry>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for entry in Archive::new(flate2::read::GzDecoder::new(tarball)).entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.push((path, bytes));
    }
    Ok(files)
}
