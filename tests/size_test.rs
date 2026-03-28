use std::mem::size_of;

use indexmap::IndexMap;
use num_bigint::BigInt;
use wqpl::{
    value::{ClosureData, FunctionData, Value},
    vm::instruction::{Capture, Instruction},
};

#[test]
fn print_size() {
    println!("Size of Instruction: {}", size_of::<Instruction>());
    println!("Size of Value: {}", std::mem::size_of::<Value>());
    println!("Size of Vec<Value>: {}", std::mem::size_of::<Vec<Value>>());
    println!("Size of String: {}", size_of::<String>());
    println!("Size of BigInt: {}", std::mem::size_of::<BigInt>());
    println!(
        "Size of IndexMap: {}",
        std::mem::size_of::<IndexMap<String, Value>>()
    );
    println!("Size of Capture: {}", size_of::<Capture>());
    println!(
        "Size of ClosureData: {}",
        std::mem::size_of::<ClosureData>()
    );
    println!(
        "Size of FunctionData: {}",
        std::mem::size_of::<FunctionData>()
    );
}
