# Installation

To try wq, you can either access the _web version_ or install the _native CLI_.

The web version is the easiest way to explore wq immediately without any setup.

The native CLI provides the full wq experience.

## The web build (wqide)

The official site for wq, [wq-pl.com](https://wq-pl.com), hosts **wqide**, the web-based version of wq.

wqide is useful for learning, experimenting, and quick mobile access.

However, it currently lacks support for advanced features such as REPL magic commands, file I/O, and advanced debugging.

## Install with Cargo

If you use Rust:

```sh
cargo install wqpl
wq -h
```

## Build from source

You can also clone the repository and build wq yourself:

```sh
git clone https://codeberg.org/wqpl/wq
cd wq
cargo build --release
```
