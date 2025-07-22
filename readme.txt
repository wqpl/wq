wq 0.2.0
(c) tttiw (l) mit
cargo test
python3 exa-t.py test
rlwrap cargo run --quiet

./doc/refcard.txt
./doc/hints.txt

INTRO
=====
//basics
    + - * / % ~ = < <= > >= #
    x:42
    y:(1;2;3);y[0]
    z:(`a:1;`b:3);z[`b]
    2*til 5
    til 3+til 2
    (1;2),(3;4)        // cat. (1;2;3;4)

//types
    int float char symbol bool list  dict        function
              "a"  `a          (1;2) (`a:1;`b:2) {[m;n]m*2+n}
    * string: list of chars "abcd" ("a";"b";"c";"d")

//fn:
    f:{x+y}            // implicit params
    f[3;4;]            // call (trailing ;)
    g:{[a;b]a*b}       // explicit params
    g[5;6;]
    //local variables inside functions
    add7:{k:7;x+k}
    a:{b:8;{b}[;]}
    //single arg functions can be called w/o brackets
    signum 10
    signum(-10)
    max(1;2;3)
//cond:
    $[true;1;2]        // 1
    $[false;1;2]       // 2
    $.[true;
      echo 1;          // 1
      echo 2]          // 2
    5<=5
    5~3                // !=. true
    3<5
//loops:
    W[i<3;i:i+1;]      // while style loop
    N[3;echo n;]       // for-i-in-range style loop. counter implicitly binds to n
//your ordinary fib:
    fibtr:{[n;a;b]$[n=0;a;fibtr[n-1;b;a+b;]]};fib:{[n]fibtr[n;0;1;]}
    fib 10

REPL CMDS
=========
\h  \v    \c    \l   \t   \d    \q   \b
help vars clear load time debug quit box
          ^ clear vars    ^ tok & ast

BUILTINS
========
abs x      // abs(-3) -> 3
neg x      // neg 2 -> -2
signum x   // signum(-2) -> -1
floor x    // floor 3.9 -> 3
ceiling x  // ceiling 3.1 -> 4
sqrt x     // sqrt 9 -> 3
exp x      // exp 1 -> 2.718...
ln x       // ln 2.718 -> 0.999...
sin cos tan sinh cosh tanh
rand[;]    // rand[;] -> 0.123
rand n     // rand 10 -> 7
rand[a;b;] // rand[1;5;] -> 3

lists
-----
count L    // count(1;2) -> 2
first L    // first(1;2;3) -> 1
last L     // last(1;2;3) -> 3
reverse L  // reverse(1;2) -> (2;1)
sum L      // sum(1;2;3) -> 6
max L      // max(1;5;2) -> 5
min L      // min(1;5;2) -> 1
avg L      // avg(1;2;3) -> 2

til n      // til 3 -> (0;1;2)
range[a;b;]// range[2;5;] -> (2;3;4)

take[n;L;] // take[2;(1;2;3);] -> (1;2)
drop[n;L;] // drop[1;(1;2;3);] -> (2;3)
where L    // where(0;1;0;1) -> (1;3)
distinct L // distinct(1;1;2) -> (1;2)
sort L     // sort(3;1;2) -> (1;2;3)
cat[a;b;]  // cat[(1;2);3;] -> (1;2;3)
flatten x  // flatten((1;2);(3;(4))) -> (1;2;3;4)

logic
-----
and[a;b]   // and[true;(true;false);] -> (true;false)
or[a;b]
not a
xor[a;b]

type
----
type x     // ->"int", "list", "dict", "function", ...
string x   // 2 -> "2"
symbol x   // "2" -> `2

io
--
echo[...]  // echo[1;2;3;4;]
