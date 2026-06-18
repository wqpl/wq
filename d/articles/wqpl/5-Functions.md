# Functions

A function is a value that can do work later.

```wq
sq:{x*x}
sq 9|echo
```

That first function uses the implicit argument `x`.

## Explicit Parameters

Use a parameter list when names make the function easier to read.

```wq
area:{[w;h]w*h}
area[6;7]|echo
```

Square brackets call a function with multiple arguments. Semicolons separate the arguments.

## Defaults And Named Arguments

Tagged parameters can have defaults.

```wq
scale:{[x;`by:2]x*by}
scale 10|echo
scale[10;`by:3]|echo
```

Named arguments may arrive out of order, which is useful for functions with options.

```wq
box:{[`w:4;`h:2]w*h}
box[`h:5;`w:3]|echo
```

## Functions In Lists

Functions are values, so they can travel into other functions.

```wq
(1;2;3)|map{x*x}|echo
(1;2;3;4;5)|filter{x%2=0}|echo
(1;2;3;4)|fold{x+y}|echo
```

That is often more direct than writing a loop.

## Capturing Nearby Names

A function can use names from around it.

```wq
make_adder:{[n]{x+n}}
add10:make_adder 10
add10 5|echo
```

The inner function remembers `n`.

## Keep

- `{x*x}` creates a one-argument function with implicit `x`.
- `{[a;b]a+b}` names parameters.
- `fn[arg1;arg2]` passes multiple arguments.
- Higher-order bfns like `map`, `filter`, and `fold` make lists feel alive.
