use std::mem::size_of;

use wqpl::vm::instruction::{Capture, Instruction};

#[test]
fn print_size() {
    println!("Size of Instruction: {}", size_of::<Instruction>());
    println!("Size of Capture: {}", size_of::<Capture>());
    println!("Size of String: {}", size_of::<String>());
}
