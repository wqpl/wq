wq 0.1.2
(c) tttiw (l) mit
cargo test
rlwrap cargo run

//basics:
    x:42
    y:(1;2;3);y[0]
    2*til 5            //(0;2;4;6;8)
    til 3+til 2        //(0;2;2)

//fn:
    f:{x+y}            // implicit params
    f[3;4;]            // call (trailing ;)
    g:{[a;b]a*b}       // explicit params
    g[5;6;]

    //single arg functions can be called w/o brackets
    signum 10
    signum(-10)
    max(1;2;3)

//cond:
    $[true;42;99]      // if
    5<=5
    5~3                // !=. true
    3<5

//your ordinary fib:
    fibtr:{[n;a;b]$[n=0;a;fibtr[n-1;b;a+b;]]}
    fib:{[n]fibtr[n;0;1;]}
    fib[10;]

//builtins:
    abs neg signum sqrt exp log floor ceiling
    count first last reverse sum max min avg
    til range
    type string
    take drop where distinct sort
    and or not

//repl cmds:
    help
    vars
    clear              // clear all vars
    quit
    load
    \h \v \c \q \l

//types:
    42                 // int
    3.14               // float
    "a"                // char
    `symbol            // symbol
    true false         // bool
    (1;2;3)            // list
    ,42                // single element list
    ()                 // empty list
