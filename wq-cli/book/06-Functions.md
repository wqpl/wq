# Functions

A function is a value that can do work later.

```wq
sq:{x*x}
sq 9
```

That function uses the first implicit parameter, `x`. Function literals without
an explicit parameter list can use `x`, `y`, and `z` for their first three
arguments.

## Explicit Parameters

Use a parameter list when names make the function easier to read.

```wq
area:{[w;h]w*h}
area[6;7]
```

Square brackets call a function with multiple arguments. Semicolons separate the arguments.

## Defaults And Named Arguments

Tagged parameters can have defaults.

<!-- wq-example {"id":"function-default-argument","cellGroup":"function-defaults"} -->
```wq
scale:{[x;`by:2]x*by}
scale 10
```

<!-- wq-example {"id":"function-named-argument","cellGroup":"function-defaults"} -->
```wq
scale[10;`by:3]
```

Named arguments can arrive out of order, which is useful for functions with options.

```wq
box:{[`w:4;`h:2]w*h}
box[`h:5;`w:3]
```

## Functions In Lists

Functions are values, so they can travel into other functions.

<!-- wq-example {"id":"function-map-list","cellGroup":"higher-order-functions"} -->
```wq
(1;2;3)|map{x*x}
```

<!-- wq-example {"id":"function-filter-list","cellGroup":"higher-order-functions"} -->
```wq
(1;2;3;4;5)|filter{x%2=0}
```

<!-- wq-example {"id":"function-fold-list","cellGroup":"higher-order-functions"} -->
```wq
(1;2;3;4)|fold{x+y}
```

`map` and `filter` call their function with `x`. `fold` calls its function with
the accumulated value as `x` and the next item as `y`.

That is often more direct than writing a loop.

## Capturing Nearby Names

A function can use names from around it.

```wq
make_adder:{[n]{x+n}}
add10:make_adder 10
add10 5
```

The inner function remembers `n`.

Ordinary closures capture nearby values. Prefix a function literal with `'`
when the function must update a captured binding by reference:

```wq
make_counter:{[]n:0;'{n+:1}}
counter:make_counter[]
(counter[];counter[])
```

The expected output is `(1;2)`. Modules use this form for exported functions
that retain private mutable state.

## Keep

- `{x*x}` creates a function that uses implicit parameter `x`.
- An implicit function can use `x`, `y`, and `z` for its first three arguments.
- `{[a;b]a+b}` names parameters.
- `fn[arg1;arg2]` passes multiple arguments.
- Higher-order builtins like `map`, `filter`, and `fold` apply functions to
  lists.
- `'{...}` captures referenced bindings so the function can update them.
