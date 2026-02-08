#![cfg_attr(target_arch = "wasm32", no_main)]

#[cfg(not(target_arch = "wasm32"))]
mod daydream;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod tshelper;
#[cfg(not(target_arch = "wasm32"))]
mod wqdb_shell;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    native::main();
}
