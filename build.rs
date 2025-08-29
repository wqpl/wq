use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

const INDEX_FILE_NAME: &str = "builtins.txt";
const INDEX_WRAP: usize = 80; // target line width for the index columns
const COL_SPACING: usize = 2; // spaces between columns
const RENAMES: &[(&str, &str)] = &[("null", "null?"), ("fexists", "fexists?"), ("m", "m?")];

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("builtins_help.rs");
    let mut file = fs::File::create(&dest_path).unwrap();

    writeln!(
        file,
        "pub fn get_builtin_help(name: &str) -> Option<&'static str> {{",
    )
    .unwrap();
    writeln!(file, "    match name {{").unwrap();

    let builtin_dir = Path::new("doc/builtins");
    let mut doc_files = Vec::new();
    if builtin_dir.exists() {
        collect_txt_files(builtin_dir, &mut doc_files);
        doc_files.sort();

        // Generate the index file before emitting the match arms,
        // then add it to the list of documented files.
        if let Some(index_path) = generate_index_file(builtin_dir, &doc_files) {
            // Only push if not already discovered by collect_txt_files
            if !doc_files.iter().any(|p| p == &index_path) {
                doc_files.push(index_path);
            }
        }
        // Ensure stable order with no duplicates (handles cases where the
        // index existed before and we also just regenerated it).
        doc_files.sort();
        doc_files.dedup();

        let renames: HashMap<&str, &str> = RENAMES.iter().copied().collect();

        for path in &doc_files {
            let stem = path.file_stem().unwrap().to_string_lossy();
            let is_index = stem.as_ref() == "builtins";
            let name = if is_index {
                "builtins"
            } else {
                renames.get(stem.as_ref()).copied().unwrap_or(stem.as_ref())
            };
            let path_str = path.to_str().unwrap().replace('\\', "/");
            writeln!(
                file,
                "        \"{name}\" => Some(include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{path_str}\"))),",
            )
            .unwrap();
        }
    }

    writeln!(file, "        _ => None,").unwrap();
    writeln!(file, "    }}").unwrap();
    writeln!(file, "}}").unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=doc/builtins");
    for path in &doc_files {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn collect_txt_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_txt_files(&path, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("txt") {
                files.push(path);
            }
        }
    }
}

/// Generate `doc/builtins/builtins.txt` with a folder-index of all builtin help files.
/// Returns the path to the generated file on success.
fn generate_index_file(root: &Path, files: &[PathBuf]) -> Option<PathBuf> {
    // Group by *relative* parent directory ("" for root, or "io", "math/adv", etc.)
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in files {
        if f.file_name()?.to_string_lossy() == INDEX_FILE_NAME {
            continue; // don't index a previous index
        }
        // Only consider *.txt that are inside root
        let rel = f.strip_prefix(root).ok()?;
        if rel.extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        let parent = rel
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let stem = rel.file_stem().unwrap().to_string_lossy().to_string();
        groups.entry(parent).or_default().push(stem);
    }

    let renames: HashMap<&str, &str> = RENAMES.iter().copied().collect();
    // Sort names within each group
    for names in groups.values_mut() {
        for name in names.iter_mut() {
            if let Some(&new_name) = renames.get(name.as_str()) {
                *name = new_name.to_string();
            }
        }
        names.sort();
    }

    // Build the text
    let mut out = String::new();
    for (group, names) in &groups {
        let title = if group.is_empty() {
            "builtins"
        } else {
            group.as_str()
        };
        let renamed = renames.get(title).unwrap_or(&title);
        out.push_str(renamed);
        out.push('\n');
        out.push_str(&"=".repeat(renamed.len()));
        out.push('\n');
        out.push_str(&format_columns(names, INDEX_WRAP, COL_SPACING));
        out.push('\n');
    }

    // Write to doc/builtins/builtins.txt
    let index_path = root.join(INDEX_FILE_NAME);
    if let Some(parent) = index_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match fs::write(&index_path, out) {
        Ok(_) => Some(index_path),
        Err(_) => None,
    }
}

/// Lay strings out in aligned columns, wrapping to the given width.
/// Uses fixed column width based on the longest item so columns align vertically.
fn format_columns(items: &[String], wrap: usize, spacing: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let max_len = items.iter().map(|s| s.len()).max().unwrap_or(0);
    let col_w = max_len + spacing;
    let cols = std::cmp::max(
        1,
        if col_w == 0 {
            1
        } else {
            (wrap + spacing) / col_w
        },
    );
    let rows = items.len().div_ceil(cols);

    let mut lines = String::new();
    for r in 0..rows {
        let mut line = String::new();
        for c in 0..cols {
            let idx = r + c * rows;
            if idx >= items.len() {
                break;
            }
            let s = &items[idx];
            if c < cols - 1 {
                // pad to column width so following columns align
                line.push_str(&pad_to(s, col_w));
            } else {
                line.push_str(s);
            }
        }
        lines.push_str(line.trim_end());
        lines.push('\n');
    }
    lines
}

fn pad_to(s: &str, width: usize) -> String {
    if s.len() >= width {
        return s.to_string();
    }
    let mut out = String::with_capacity(width);
    out.push_str(s);
    for _ in 0..(width - s.len()) {
        out.push(' ');
    }
    out
}
