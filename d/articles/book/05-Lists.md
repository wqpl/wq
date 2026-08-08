# Lists

Lists are ordered containers written with parentheses and semicolons.

<!-- wq-example {"id":"list-intro-ints","cellGroup":"list-intro"} -->
```wq
(1;2;3)
```

<!-- wq-example {"id":"list-intro-colors","cellGroup":"list-intro"} -->
```wq
("red";"green";"blue")
```

Semicolons separate items.

## Atoms and Singleton Lists

`(1)` groups the atom `1`. A leading comma creates a one-item list.

<!-- wq-example {"id":"list-grouped-atom","cellGroup":"one-or-list"} -->
```wq
(1)
```

<!-- wq-example {"id":"list-enlisted-atom","cellGroup":"one-or-list"} -->
```wq
,1
```

The empty list is `()`.

```wq
empty:()
#empty
```

`#value` returns its length. This block returns `0`.

## Indexing

Indexes start at zero. Negative indexes count from the end.

<!-- wq-example {"id":"list-index-first","cellGroup":"list-indexes"} -->
```wq
xs:(10;20;30;40)
xs[0]
```

<!-- wq-example {"id":"list-index-second","cellGroup":"list-indexes"} -->
```wq
xs[1]
```

<!-- wq-example {"id":"list-index-last","cellGroup":"list-indexes"} -->
```wq
xs[-1]
```

Postfix indexing is common in tight code:

```wq
xs:(10;20;30)
xs 1
```

## Ranges And Slices

`a..b` stops before `b`. `a..=b` includes `b`. A middle point sets the stride.

<!-- wq-example {"id":"range-exclusive","cellGroup":"range-shapes"} -->
```wq
1..5
```

<!-- wq-example {"id":"range-inclusive","cellGroup":"range-shapes"} -->
```wq
1..=5
```

<!-- wq-example {"id":"range-stride","cellGroup":"range-shapes"} -->
```wq
0..2..=10
```

Ranges also slice lists.

<!-- wq-example {"id":"slice-forward","cellGroup":"list-slices"} -->
```wq
xs:(10;20;30;40;50)
xs[1..4]
```

<!-- wq-example {"id":"slice-negative","cellGroup":"list-slices"} -->
```wq
xs[-3..=-1]
```

## Shape, Depth, and Axes

Nested lists can represent rows, grids and higher-dimensional data. An axis is
one direction through a uniform nested list. Axis `0` is the outermost list,
axis `1` is the next list inward, and so on.

`R` is the short name for `reshape`. It flattens the input, then fills a new
shape whose numbers give the length of each axis from outermost to innermost:

<!-- wq-example {"id":"list-reshape-matrix","cellGroup":"list-shape-depth","expect":{"value":"((1;2;3);(4;5;6))"}} -->
```wq
matrix:R[1..=6;(2;3)]
matrix
```

This shape creates `2` rows with `3` items in each row. With nonempty input, `R`
cycles through the flattened input if the requested shape needs more items.

`shape` reports those axis lengths. `depth` reports how many container layers
lead from the outer value to its deepest atom:

<!-- wq-example {"id":"list-matrix-shape","cellGroup":"list-shape-depth","expect":{"value":"(2;3)"}} -->
```wq
shape matrix
```

<!-- wq-example {"id":"list-matrix-depth","cellGroup":"list-shape-depth","expect":{"value":"2"}} -->
```wq
depth matrix
```

Atoms have depth `0`, flat lists have depth `1`, and this matrix has depth `2`.
For a ragged list, `shape` stops after the last uniform axis while `depth` still
follows the deepest branch.

`TP` is the short name for `transpose`. On a matrix, it swaps the row and column
axes:

<!-- wq-example {"id":"list-transpose-matrix","cellGroup":"list-shape-depth","expect":{"value":"((1;4);(2;5);(3;6))"}} -->
```wq
TP matrix
```

The result has shape `(3;2)`: three rows with two items in each row.

## Unpack A Shape

A list-shaped assignment target unpacks by position:

```wq
point:(10;20)
(x;y):point
x+y
```

Patterns can nest. `...` skips the middle when only the ends matter:

```wq
(name;(r;g;b)):("sky";(90;140;255))
(head;...;tail):(1;2;3;4)
(name;r;g;b;head;tail)
```

The source runs once. wq validates every requested position before writing the
targets, so assignment is atomic.

## Mutating Lists

List indexes can be assigned. The path starts from a binding such as `xs`.

```wq
xs:(10;20;30)
xs[1]:99
xs
```

`[!]` pops from the end. `[!i]` removes at an index. `[!i]:v` inserts there.
Bang indexing acts directly on a binding, and insertion uses plain `:`.

<!-- wq-example {"id":"list-pop-value","cellGroup":"list-pop"} -->
```wq
stack:()
stack,:10
stack,:20
stack[!]
```

<!-- wq-example {"id":"list-after-pop","cellGroup":"list-pop"} -->
```wq
stack
```

```wq
xs:(10;30)
xs[!1]:20
xs
```

## Summary

- `(a;b;c)` is a list.
- `(a)` groups `a`. `,a` enlists one item.
- `xs[i]` indexes. `xs[a..b]` slices.
- `shape` reports axis lengths. `depth` counts container layers.
- `R` reshapes values. `TP` transposes uniform nested lists.
- `(a;b):value` unpacks a list by position.
- Index assignment and bang indexing mutate a bound list.
- Continue to **Functions** to transform lists without manual loops.
