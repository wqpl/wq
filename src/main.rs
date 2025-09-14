#![cfg_attr(target_arch = "wasm32", no_main)]

mod platforms;
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use platforms::native::start;
    start();
}
