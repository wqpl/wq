```plaintext
wq (c)tttiw (l)MIT
https://wq-pl.com
https://codeberg.org/wqpl
cargo install wqpl
```

```wq
fib:{(f_:{$[x=0;y;f_[x-1;z;y+z]]})[x;0;1]}
fib 9999
{_:{$.[x=0;@r(0;1)];(a;b):_ floor[x/2];c:a*(2*b-a);d:a^2+b^2;$[x%2=0;(c;d);(d;c+d)]};_[x]0}99999
```
