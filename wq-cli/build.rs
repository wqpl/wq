use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn main() {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let rustc_out = Command::new(&rustc)
        .arg("-vV")
        .output()
        .expect("Failed to execute rustc -vV");
    assert!(rustc_out.status.success(), "rustc -vV failed");
    let rustc_out_text = String::from_utf8(rustc_out.stdout).expect("Invalid UTF-8 from rustc");
    let rustc_version = rustc_out_text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let mut host = String::new();
    let mut llvm = String::new();
    for line in rustc_out_text.lines() {
        if let Some(v) = line.strip_prefix("host:") {
            host = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("LLVM version:") {
            llvm = v.trim().to_string();
        }
    }

    println!("cargo:rustc-env=RUSTC_VERSION={}", rustc_version);
    println!("cargo:rustc-env=RUSTC_HOST={}", host);
    println!("cargo:rustc-env=RUSTC_LLVM_VERSION={}", llvm);

    // profile =========================================================================================
    if let Ok(opt) = std::env::var("OPT_LEVEL") {
        println!("cargo:rustc-env=BUILD_OPT_LEVEL={opt}");
    }
    if let Ok(profile) = std::env::var("PROFILE") {
        println!("cargo:rustc-env=BUILD_PROFILE={profile}");
    }

    generate_embedded_book();
}

fn generate_embedded_book() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be available"),
    );
    let catalog_path = manifest_dir.join("book/catalog.json");
    println!("cargo:rerun-if-changed={}", catalog_path.display());

    let catalog_source = fs::read_to_string(&catalog_path).expect("book catalog must be readable");
    let catalog: serde_json::Value =
        serde_json::from_str(&catalog_source).expect("book catalog must contain valid JSON");
    let title = catalog["title"]
        .as_str()
        .expect("book catalog must have a title");
    let description = catalog["description"]
        .as_str()
        .expect("book catalog must have a description");
    let chapters = catalog["chapters"]
        .as_array()
        .expect("book catalog must have a chapters array");

    let mut generated = String::new();
    writeln!(generated, "pub(super) const BOOK_TITLE: &str = {title:?};")
        .expect("writing generated book title must succeed");
    writeln!(
        generated,
        "pub(super) const BOOK_DESCRIPTION: &str = {description:?};"
    )
    .expect("writing generated book description must succeed");
    writeln!(
        generated,
        "pub(super) static BOOK_CHAPTERS: &[EmbeddedChapter] = &["
    )
    .expect("writing generated chapter list must succeed");

    for chapter in chapters {
        let slug = chapter["slug"]
            .as_str()
            .expect("every book chapter must have a slug");
        let chapter_title = chapter["title"]
            .as_str()
            .expect("every book chapter must have a title");
        let file = chapter["file"]
            .as_str()
            .expect("every book chapter must have a file");
        let chapter_description = chapter["description"]
            .as_str()
            .expect("every book chapter must have a description");
        let optional = chapter["optional"].as_bool().unwrap_or(false);
        let chapter_path = manifest_dir.join("book").join(file);
        println!("cargo:rerun-if-changed={}", chapter_path.display());
        let chapter_path = chapter_path
            .to_str()
            .expect("book chapter paths must be valid UTF-8");

        writeln!(
            generated,
            "    EmbeddedChapter {{ slug: {slug:?}, title: {chapter_title:?}, description: {chapter_description:?}, content: include_str!({chapter_path:?}), optional: {optional} }},"
        )
        .expect("writing generated chapter entry must succeed");
    }
    writeln!(generated, "];").expect("finishing generated chapter list must succeed");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be available"));
    fs::write(out_dir.join("book.rs"), generated)
        .expect("generated book registry must be writable");
}
