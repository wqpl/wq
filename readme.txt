wq (c) tttiw (l) mit
--------------------------
wq              repl
wq -h           usage help
echo '\h' | wq  refcard
--------------------------
cargo test
python3 exa-t.py gen
cargo run
--------------------------
fibtr:{[n;a;b]$[n=0;a;fibtr[n-1;b;a+b;]]}
fib:{fibtr[x;0;1;]}
echo fib 90

