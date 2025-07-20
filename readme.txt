wq 0.1.2
(c) tttiw (l) mit
cargo test
rlwrap cargo run

//basics:
    x:42
    y:(1;2;3);y[0]
    2*til 5            // (0;2;4;6;8)
    til 3+til 2        // (0;2;2)
    (1;2),(3;4)        // cat. (1;2;3;4)
    + - * / %

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
    $[true;42;99]      // if
    5<=5
    5~3                // !=. true
    3<5

//loops:
    W[i<3;i:i+1;]      // while style loop
    N[3;echo n;]       // for-i-in-range style loop. counter implicitly binds to n

//your ordinary fib:
    fibtr:{[n;a;b]$[n=0;a;fibtr[n-1;b;a+b;]]}
    fib:{[n]fibtr[n;0;1;]}
    fib[10;]

//builtins:
    abs neg signum sqrt exp log floor ceiling
    count first last reverse sum max min avg
    rand sin cos tan sinh cosh tanh
    til range type string
    take drop where distinct sort
    cat flatten and or not xor echo

//repl cmds:
    help
    vars
    clear              // clear all vars
    load
    time
    debug on debug off // toggle ast printing
    quit
    \h \v \c \l \t \q

//types:
    42                 // int
    3.14               // float
    "a"                // char
    "abc"              // string (list of chars)
    `symbol            // symbol
    true false         // bool
    (1;2;3)            // list
    ,42                // single element list
    ()                 // empty list
