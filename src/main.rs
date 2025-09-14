#![cfg_attr(target_arch = "wasm32", no_main)]

mod native;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    native::main();
}
