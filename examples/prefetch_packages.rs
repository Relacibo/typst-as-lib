//! Fetches `@preview` packages ahead of compilation and feeds them into the
//! engine as static resolvers.
//!
//! Typst's `World` is synchronous — it only does *blocking* IO, which
//! browser WASM can't provide mid-compile (the event loop can't serve new
//! data while the compile runs). Hosts therefore fetch packages *ahead* of
//! the compile:
//!
//! 1. **Scan** the sources for package imports, transitively — packages
//!    import packages. Skip malformed imports: never panic on
//!    template-controlled input.
//! 2. **Fetch** `https://packages.typst.org/preview/{name}-{version}.tar.gz`
//!    with whatever client the host prefers (`fetch()` + IndexedDB in the
//!    browser, tokio/reqwest-async on servers).
//! 3. **Extract** the tar.gz (`flate2` + `binstall-tar`).
//! 4. **Feed** the files into the engine: build `FileId`s via
//!    `RootedPath::new(VirtualRoot::Package(spec), vpath).intern()`, register
//!    `.typ` files with `with_static_source_file_resolver` and everything
//!    else with `with_static_file_resolver` — *before* any other resolver,
//!    since resolver order is try order.
//!
//! For servers that prefetch per compile, a custom `FileResolver`
//! (`Mutex<HashMap<FileId, …>>`) replaces the static resolvers.
//!
//! Run: `cargo run --features packages,ureq --example prefetch_packages`

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;

use binstall_tar::Archive;
use typst::syntax::package::PackageSpec;
use typst::syntax::{RootedPath, SyntaxNode, VirtualPath, VirtualRoot, ast};
use typst_as_lib::TypstEngine;
use typst_layout::PagedDocument;

static FONT: &[u8] = include_bytes!("fonts/texgyrecursor-regular.otf");
static TEMPLATE: &str = r#"#import "@preview/tablex:0.0.9": tablex
#tablex(columns: (1fr, 1fr), [Prefetched], [packages!])
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1) Scan: find package imports in the template, transitively.
    let mut found: HashSet<PackageSpec> = HashSet::new();
    let mut queue: Vec<PackageSpec> = Vec::new();
    scan_packages(TEMPLATE, &mut found, &mut queue);

    // 2) Fetch + extract, transitively: packages import packages.
    let mut package_sources: Vec<(typst::syntax::FileId, String)> = Vec::new();
    let mut package_binaries: Vec<(typst::syntax::FileId, Vec<u8>)> = Vec::new();
    let mut fetched: HashSet<PackageSpec> = HashSet::new();
    while let Some(spec) = queue.pop() {
        if !fetched.insert(spec.clone()) {
            continue;
        }
        let tarball = fetch_package(&spec)?;
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
    println!(
        "prefetched {} package files",
        package_sources.len() + package_binaries.len()
    );

    // 3) Feed: package resolvers first (resolver order = try order), then the
    //    template file. The engine compiles without any blocking IO.
    let engine = TypstEngine::builder()
        .with_static_source_file_resolver(package_sources)
        .with_static_file_resolver(package_binaries)
        .with_static_source_file_resolver([("main.typ", TEMPLATE)])
        .fonts([FONT])
        .build();

    let warned = engine.compile("main.typ");
    for warning in &warned.warnings {
        eprintln!("typst warning: {}", warning.message);
    }
    let doc: PagedDocument = warned.output?;
    let mut text = String::new();
    for page in doc.pages() {
        collect_text(&page.frame, &mut text);
    }
    println!("pages: {}, text: {:?}", doc.pages().len(), text);
    assert!(
        text.contains("Prefetched"),
        "expected table text, got: {text}"
    );
    println!("compiled with prefetched packages ✓");
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
            if let Some(str_node) = child.cast::<ast::Str>() {
                let import = str_node.get();
                if !import.starts_with('@') {
                    continue;
                }
                let Some(spec) = parse_spec(&import) else {
                    continue;
                };
                if found.insert(spec.clone()) {
                    queue.push(spec);
                }
            }
        }
        return;
    }
    for child in node.children() {
        walk_imports(child, found, queue);
    }
}

/// Parses `@namespace/name:version`, tolerating a path suffix
/// (`@preview/pkg:0.1.0/chapter.typ`), which imports from within packages use.
fn parse_spec(text: &str) -> Option<PackageSpec> {
    if let Ok(spec) = text.parse() {
        return Some(spec);
    }
    let mut end = text.len();
    while let Some(position) = text[..end].rfind('/') {
        if let Ok(spec) = text[..position].parse() {
            return Some(spec);
        }
        end = position;
    }
    None
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
    let response = ureq::get(&url)
        .call()
        .map_err(Box::<dyn std::error::Error>::from)?;
    Ok(response.into_body().read_to_vec()?)
}

/// Collects text from a frame and all nested frames (tables/grids render as
/// nested groups).
fn collect_text(frame: &typst::layout::Frame, out: &mut String) {
    for (_, item) in frame.items() {
        use typst::layout::FrameItem;
        match item {
            FrameItem::Text(text) => out.push_str(&text.text),
            FrameItem::Group(group) => {
                collect_text(&group.frame, out);
            }
            _ => {}
        }
    }
}

type TarEntry = (PathBuf, Vec<u8>);

fn extract(tarball: &[u8]) -> Result<Vec<TarEntry>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for entry in Archive::new(flate2::read::GzDecoder::new(tarball))
        .entries()
        .map_err(Box::<dyn std::error::Error>::from)?
    {
        let mut entry = entry.map_err(Box::<dyn std::error::Error>::from)?;
        let path = entry
            .path()
            .map_err(Box::<dyn std::error::Error>::from)?
            .to_path_buf();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(Box::<dyn std::error::Error>::from)?;
        files.push((path, bytes));
    }
    Ok(files)
}
