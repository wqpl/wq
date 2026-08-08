# Higher-Order Builtins

A higher-order builtin receives a function as a value and decides how to call
it. These builtins cover common list transformations without a manual loop.

## Transform Each Item

`map[xs;f]` calls `f` once for each item and collects the results. `M` is its
short name.

<!-- wq-example {"id":"higher-order-map","expect":{"value":"(1;4;9)"}} -->
```wq
map[(1;2;3);{x*x}]
```

By default, `map` works at depth `1`, the immediate items of the outer
container. Pass a different depth as the third argument when the function
should receive another layer:

<!-- wq-example {"id":"higher-order-map-rows","cellGroup":"map-depths","expect":{"value":"(3;7)"}} -->
```wq
matrix:((1;2);(3;4))
map[matrix;{sum x}]
```

<!-- wq-example {"id":"higher-order-map-items","cellGroup":"map-depths","expect":{"value":"((1;4);(9;16))"}} -->
```wq
M[matrix;{x*x};2]
```

The first call gives each row to the function. The second uses depth `2`, so
`M` gives it each number.

## Keep Matching Items

`filter[xs;predicate]` keeps items for which the predicate returns `T`:

<!-- wq-example {"id":"higher-order-filter","expect":{"value":"(2;4)"}} -->
```wq
filter[(1;2;3;4;5);{x%2=0}]
```

The predicate must return a bool. `any` and `all` answer related questions
without building a new list:

<!-- wq-example {"id":"higher-order-any","cellGroup":"predicate-questions","expect":{"value":"T"}} -->
```wq
any[(1;2;3);{x>2}]
```

<!-- wq-example {"id":"higher-order-all","cellGroup":"predicate-questions","expect":{"value":"T"}} -->
```wq
all[(1;2;3);{x>0}]
```

`any` stops after the first `T`; `all` stops after the first `F`.

## Pair Corresponding Items

`zip` pairs items at matching positions:

<!-- wq-example {"id":"higher-order-zip","cellGroup":"zip-and-zipw","expect":{"value":"((1;10);(2;20);(3;30))"}} -->
```wq
zip[(1;2;3);(10;20;30)]
```

`zipw` calls a two-argument function for each pair instead. The function
receives the left item as `x` and the right item as `y`:

<!-- wq-example {"id":"higher-order-zipw","cellGroup":"zip-and-zipw","expect":{"value":"(11;22;33)"}} -->
```wq
zipw[(1;2;3);(10;20;30);{x+y}]
```

Both builtins accept an optional depth after their usual arguments.

## Fold and Scan

`fold` combines a list into one result from left to right. Its function receives
the accumulated result as `x` and the next item as `y`:

<!-- wq-example {"id":"higher-order-fold","cellGroup":"fold-and-scan","expect":{"value":"10"}} -->
```wq
fold[(1;2;3;4);{x+y}]
```

`scan` performs the same steps but keeps every running result:

<!-- wq-example {"id":"higher-order-scan","cellGroup":"fold-and-scan","expect":{"value":"(1;3;6;10)"}} -->
```wq
scan[(1;2;3;4);{x+y}]
```

Pass a third argument to either builtin when the accumulation needs an explicit
initial value.

## Summary

- `map` and its alias `M` transform items at a chosen depth.
- `filter` keeps matching items; `any` and `all` answer predicate questions.
- `zip` creates pairs; `zipw` combines corresponding items with a function.
- `fold` returns one accumulated result; `scan` keeps the running results.
- Continue to **Dicts** to work with named fields and ordered entries.
