use std::env;
use std::process::Command;

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
}
