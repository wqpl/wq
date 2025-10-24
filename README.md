```plaintext
wq (c)tttiw (l)MIT

wq               repl
wq -h            usage help
echo '!h'|wq     refcard
```

```sh
cargo run --release
```

```wq
fib:{(f_:{$[x=0;y;f_[x-1;z;y+z]]})[x;0;1]}
fib 90
```
