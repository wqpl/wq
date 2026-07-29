# Lists

Lists are the main shape of wq data. They are small to write and pleasant to push through functions.

<!-- wq-example {"id":"list-intro-ints","cellGroup":"list-intro"} -->
```wq
(1;2;3)
```

<!-- wq-example {"id":"list-intro-colors","cellGroup":"list-intro"} -->
```wq
("red";"green";"blue")
```

Semicolons separate items. Spaces do not.

## One Thing, Or A List?

`(1)` is just `1`. Use leading comma when you want a one-item list.

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

`#value` returns its length, so the expected output is `0`.

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

The source runs once. wq validates every requested position before writing any
target, so an invalid pattern does not leave partial assignments behind.

## Mutating Lists

List indexes can be assigned. The path must start from a binding such as `xs`;
a temporary result cannot be an assignment target.

```wq
xs:(10;20;30)
xs[1]:99
xs
```

`[!]` pops from the end. `[!i]` removes at an index, and `[!i]:v` inserts
there. Bang indexing acts directly on a binding, and insertion uses plain `:`
rather than an operator-colon form.

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

## Keep

- `(a;b;c)` is a list.
- `(a)` is just `a`; `,a` enlists one item.
- `xs[i]` indexes; `xs[a..b]` slices.
- `(a;b):value` unpacks a list by position.
- Lists are mutable when you choose to mutate them.
- Continue to **Functions** to transform lists without manual loops.
