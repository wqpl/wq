   wq (c) tttiw (l) mit
==========================
wq              repl
wq -h           usage help
echo '\h' | wq  refcard
==========================
/doc
CONTRIBUTING.txt
==========================
fibtr:{[n;a;b]$[n=0;a;fibtr[n-1;b;a+b;]]}
fib:{fibtr[x;0;1;]}
echo fib 90
