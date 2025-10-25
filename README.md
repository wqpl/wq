```plaintext
wq (c)tttiw (l)MIT
https://codeberg.org/wqpl

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

{_:{$.[x=0;@r(0;1)];(a;b):_ floor[x/2];c:a*(2*b-a);d:a^2+b^2;$[x%2=0;(c;d);(d;c+d)]};_[x]0}9999
```
