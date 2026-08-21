# Installation

To try wq, you can either access the _web version_ or install the _native CLI_.

The web version is the easiest way to explore wq immediately without any setup.

The native CLI provides the full wq experience.

## The web build (wqide)

The official site for wq, [wq-pl.com](https://wq-pl.com), hosts **wqide**, the web-based version of wq.

wqide is useful for learning, experimenting, and quick mobile access.

The browser sandbox still limits file I/O and some advanced REPL commands. The REPL includes wqdb with entry pauses, stepping, source-line breakpoints, stack and binding inspection, and symbol tracking.

## Install with Cargo

If you use Rust:

```sh
cargo install wq-cli
wq -h
```

## Build from source

You can also clone the repository and build wq yourself:

```sh
git clone https://github.com/wqpl/wq
cd wq
cargo build -p wq-cli --release
```
