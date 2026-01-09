#[cfg(feature = "package-bundling")]
fn main() {
    use std::env;
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let pkg_dir = out_dir.join("typst_packages");

    // Priority 1: Environment variable (highest priority - explicit override)
    // Priority 2: Cargo.toml metadata (project configuration)
    let template_dir = env::var("TYPST_TEMPLATE_DIR")
        .ok()
        .or_else(|| {
            // Read Cargo.toml to get metadata
            let cargo_toml_path =
                Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml");
            fs::read_to_string(cargo_toml_path)
                .ok()
                .and_then(|content| content.parse::<toml::Table>().ok())
                .and_then(|manifest| {
                    manifest
                        .get("package")?
                        .get("metadata")?
                        .get("typst-as-lib")?
                        .get("template-dir")?
                        .as_str()
                        .map(|s| s.to_string())
                })
        })
        .unwrap_or_else(|| {
            eprintln!(
                "\n\
                ERROR: Template directory not configured for package-bundling feature.\n\
                \n\
                Choose ONE of the following solutions:\n\
                \n\
                Option 1 (Recommended): Add to Cargo.toml\n\
                \n\
                  [package.metadata.typst-as-lib]\n\
                  template-dir = \"./templates\"\n\
                \n\
                Option 2: Set environment variable\n\
                \n\
                  export TYPST_TEMPLATE_DIR=./templates\n\
                  cargo build\n\
                \n\
                Option 3: Use .cargo/config.toml\n\
                \n\
                  [env]\n\
                  TYPST_TEMPLATE_DIR = \"./templates\"\n\
            "
            );
            std::process::exit(1);
        });

    println!("cargo:rerun-if-env-changed=TYPST_TEMPLATE_DIR");
    println!("cargo:rerun-if-changed={}", template_dir);

    let packages = extract_packages(&template_dir);

    if packages.is_empty() {
        eprintln!("No packages found in templates");
        fs::create_dir_all(&pkg_dir).ok();
    } else {
        download_packages(&packages, &pkg_dir);
    }

    println!(
        "cargo:rustc-env=TYPST_BUNDLED_PACKAGES_DIR={}",
        pkg_dir.display()
    );

    fn is_valid_identifier(s: &str) -> bool {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    }

    fn is_valid_version(s: &str) -> bool {
        !s.is_empty() && s.chars().all(|c| c.is_numeric() || c == '.')
    }

    fn parse_package_specifier(path: &str) -> Option<(String, String, String)> {
        // Package imports start with @
        if !path.starts_with('@') {
            return None;
        }

        let path = &path[1..]; // Remove @

        // Split namespace/name:version
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 2 {
            return None;
        }

        let namespace = parts[0].to_string();
        let name_version = parts[1];

        // Split name and version
        let nv_parts: Vec<&str> = name_version.split(':').collect();
        if nv_parts.len() != 2 {
            return None;
        }

        let name = nv_parts[0].to_string();
        let version = nv_parts[1].to_string();

        // Validate format
        if !is_valid_identifier(&namespace)
            || !is_valid_identifier(&name)
            || !is_valid_version(&version)
        {
            return None;
        }

        Some((namespace, name, version))
    }

    fn parse_packages_from_source(content: &str) -> Result<Vec<(String, String, String)>, String> {
        #[allow(unused_imports)]
        use typst_syntax::{
            Source,
            ast::{AstNode, Expr, Markup},
        };

        // Parse source into AST
        let source = Source::detached(content);
        let root_node = source.root();

        // Cast SyntaxNode to Markup AST node
        // AstNode trait must be in scope for cast method
        let root: Markup = match root_node.cast() {
            Some(markup) => markup,
            None => return Err("Failed to cast root node to Markup".to_string()),
        };

        let mut packages = Vec::new();

        // Iterate through all expressions
        for expr in root.exprs() {
            // Look for import expressions
            if let Expr::ModuleImport(import) = expr {
                // Extract the import source
                let source_expr = import.source();

                // Check if source is a string literal
                if let Expr::Str(str_node) = source_expr {
                    let import_path = str_node.get();

                    // Parse package specifier
                    if let Some(pkg) = parse_package_specifier(&import_path) {
                        packages.push(pkg);
                    }
                }
            }
        }

        Ok(packages)
    }

    fn extract_packages(dir: &str) -> Vec<(String, String, String)> {
        use std::collections::HashSet;
        use walkdir::WalkDir;

        let mut packages = HashSet::new();

        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "typ"))
        {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                // Parse with typst-syntax
                match parse_packages_from_source(&content) {
                    Ok(found_packages) => {
                        packages.extend(found_packages);
                    }
                    Err(e) => {
                        // Log but don't fail - graceful degradation
                        eprintln!(
                            "Failed to parse {}: {}",
                            entry.path().display(),
                            e
                        );
                    }
                }
            }
        }

        packages.into_iter().collect()
    }

    fn download_packages(packages: &[(String, String, String)], dest: &Path) {
        use std::collections::{HashSet, VecDeque};

        fs::create_dir_all(dest).unwrap();
        let mut failed_packages = Vec::new();
        let mut to_download = VecDeque::from(packages.to_vec());
        let mut downloaded = HashSet::new();

        while let Some((namespace, name, version)) = to_download.pop_front() {
            // Skip if already downloaded or attempted
            let pkg_key = format!("{}/{}/{}", namespace, name, version);
            if !downloaded.insert(pkg_key.clone()) {
                continue;
            }

            let pkg_dir = dest.join(&namespace).join(&name).join(&version);

            // Caching: skip if exists (but still check dependencies)
            if pkg_dir.exists() {
                eprintln!("Cached: {}/{}-{}", namespace, name, version);
            } else {
                eprintln!(
                    "Downloading: {}/{}-{}",
                    namespace, name, version
                );

                let url = format!(
                    "https://packages.typst.org/{}/{}-{}.tar.gz",
                    namespace, name, version
                );

                match download_and_extract(&url, &pkg_dir) {
                    Ok(_) => eprintln!("✓ {}/{}-{}", namespace, name, version),
                    Err(e) => {
                        eprintln!(
                            "✗ Failed to download {}/{}-{}: {}",
                            namespace, name, version, e
                        );
                        failed_packages.push(format!("{}/{}-{}", namespace, name, version));
                        continue;
                    }
                }
            }

            // 1. Parse typst.toml for explicit dependencies
            let toml_path = pkg_dir.join("typst.toml");
            if let Ok(content) = fs::read_to_string(&toml_path)
                && let Ok(manifest) = content.parse::<toml::Table>()
                && let Some(deps) = manifest.get("package").and_then(|p| p.get("dependencies"))
                && let Some(deps_table) = deps.as_table()
            {
                for (dep_name, dep_value) in deps_table {
                    if let Some(dep_str) = dep_value.as_str()
                        && let Some((dep_ns, dep_ver)) = dep_str.split_once(':')
                    {
                        to_download.push_back((
                            dep_ns.to_string(),
                            dep_name.clone(),
                            dep_ver.to_string(),
                        ));
                    }
                }
            }

            // 2. Scan package's .typ files for implicit dependencies
            let pkg_deps = extract_packages(pkg_dir.to_str().unwrap());
            for (dep_ns, dep_name, dep_ver) in pkg_deps {
                to_download.push_back((dep_ns, dep_name, dep_ver));
            }
        }

        // Abort build if any packages failed to download
        if !failed_packages.is_empty() {
            panic!(
                "Failed to download {} package(s):\n  - {}\n\n\
                Please check your internet connection and try again.\n\
                Downloaded packages are cached in OUT_DIR and won't be re-downloaded.",
                failed_packages.len(),
                failed_packages.join("\n  - ")
            );
        }
    }

    #[cfg(feature = "ureq")]
    fn download_and_extract(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let response = ureq::get(url).call()?;
        let (_, body) = response.into_parts();
        let mut bytes = Vec::new();
        body.into_reader().read_to_end(&mut bytes)?;
        extract_tar_gz(&bytes, dest)
    }

    #[cfg(all(not(feature = "ureq"), feature = "reqwest"))]
    fn download_and_extract(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::blocking::Client::new();
        let bytes = client.get(url).send()?.bytes()?.to_vec();
        extract_tar_gz(&bytes, dest)
    }

    fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
        use binstall_tar::Archive;
        use flate2::read::GzDecoder;

        fs::create_dir_all(dest)?;
        let gz = GzDecoder::new(bytes);
        let mut archive = Archive::new(gz);
        archive.unpack(dest)?;
        Ok(())
    }
}

#[cfg(not(feature = "package-bundling"))]
fn main() {
    // No-op when feature is disabled
}

#[cfg(all(test, feature = "package-bundling"))]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_specifier_valid() {
        assert_eq!(
            parse_package_specifier("@preview/cetz:0.3.2"),
            Some((
                "preview".to_string(),
                "cetz".to_string(),
                "0.3.2".to_string()
            ))
        );
        assert_eq!(
            parse_package_specifier("@preview/polylux:0.3.1"),
            Some((
                "preview".to_string(),
                "polylux".to_string(),
                "0.3.1".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_package_specifier_no_at_sign() {
        assert_eq!(parse_package_specifier("preview/cetz:0.3.2"), None);
    }

    #[test]
    fn test_parse_package_specifier_invalid_format() {
        assert_eq!(parse_package_specifier("@preview"), None);
        assert_eq!(parse_package_specifier("@preview/cetz"), None);
        assert_eq!(parse_package_specifier("@/cetz:0.3.2"), None);
        assert_eq!(parse_package_specifier("@preview/:0.3.2"), None);
    }

    #[test]
    fn test_parse_packages_from_source_simple() {
        let content = r#"#import "@preview/cetz:0.3.2""#;
        let packages = parse_packages_from_source(content).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(
            packages[0],
            (
                "preview".to_string(),
                "cetz".to_string(),
                "0.3.2".to_string()
            )
        );
    }

    #[test]
    fn test_parse_packages_ignores_comments() {
        let content = r#"
        // This should be ignored: @preview/fake:1.0.0
        /* Also ignored: @preview/another:2.0.0 */
        #import "@preview/cetz:0.3.2"
        "#;
        let packages = parse_packages_from_source(content).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].1, "cetz");
    }

    #[test]
    fn test_parse_packages_ignores_strings() {
        let content = r#"
        #let description = "Uses @preview/fake:1.0.0 for testing"
        #import "@preview/cetz:0.3.2"
        "#;
        let packages = parse_packages_from_source(content).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].1, "cetz");
    }

    #[test]
    fn test_parse_packages_multiple_imports() {
        let content = r#"
        #import "@preview/cetz:0.3.2"
        #import "@preview/polylux:0.3.1"
        "#;
        let packages = parse_packages_from_source(content).unwrap();
        assert_eq!(packages.len(), 2);
    }

    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("preview"));
        assert!(is_valid_identifier("cetz"));
        assert!(is_valid_identifier("my-package"));
        assert!(is_valid_identifier("my_package"));
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("my package"));
    }

    #[test]
    fn test_is_valid_version() {
        assert!(is_valid_version("0.3.2"));
        assert!(is_valid_version("1.0.0"));
        assert!(is_valid_version("2.1"));
        assert!(!is_valid_version(""));
        assert!(!is_valid_version("1.0.0-beta"));
    }
}
