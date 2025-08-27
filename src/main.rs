#![cfg_attr(target_arch = "wasm32", no_main)]

#[cfg(not(target_arch = "wasm32"))]
use wq::platforms::native::start;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    start();
}
