use std::{env, fs, io::Write, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("builtins_help.rs");
    let mut file = fs::File::create(&dest_path).unwrap();

    writeln!(
        file,
        "pub fn get_builtin_help(name: &str) -> Option<&'static str> {{"
    )
    .unwrap();
    writeln!(file, "    match name {{").unwrap();

    let builtin_dir = Path::new("doc/builtins");
    if builtin_dir.exists() {
        let mut entries: Vec<_> = fs::read_dir(builtin_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("txt") {
                let name = path.file_stem().unwrap().to_string_lossy();
                let path_str = path.to_str().unwrap().replace('\\', "/");
                writeln!(
                    file,
                    "        \"{name}\" => Some(include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{path_str}\"))),",
                    name = name,
                    path_str = path_str
                ).unwrap();
            }
        }
    }

    writeln!(file, "        _ => None,").unwrap();
    writeln!(file, "    }}").unwrap();
    writeln!(file, "}}").unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=doc/builtins");
    if Path::new("doc/builtins").exists() {
        for entry in fs::read_dir("doc/builtins").unwrap() {
            println!("cargo:rerun-if-changed={}", entry.unwrap().path().display());
        }
    }
}
