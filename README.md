```plaintext
wq (c) tttiw (l) MIT
https://wq-pl.com
https://github.com/wqpl
cargo install wq-cli
```

```wq
P:-1|arccos
f:{[x]n:#x
  $.[n=0;raise"empty input"]
  r:,0;W[#r<n;r:r*2,r*2+1]
  $.[#r~n;raise"length must be a power of 2"]
  a:x r+0i;l:2
  W[l<=n;h:l/%2;k:0..n%l<h|where;u:a k
    v:exp(-2i*P/l*0..h)|R(n/%2)|*a[k+h]
    (a[k];a[k+h]):(u+v;u-v);l*:2];a}
```
