# Lists

Lists are the main shape of wq data. They are small to write and pleasant to push through functions.

```wq
(1;2;3)|echo
("red";"green";"blue")|echo
```

Semicolons separate items. Spaces do not.

## One Thing, Or A List?

`(1)` is just `1`. Use leading comma when you want a one-item list.

```wq
(1)|echo
,1|echo
```

The empty list is `()`.

```wq
empty:()
#empty|echo
```

## Indexing

Indexes start at zero. Negative indexes count from the end.

```wq
xs:(10;20;30;40)
xs[0]|echo
xs[1]|echo
xs[-1]|echo
```

Postfix indexing is common in tight code:

```wq
xs:(10;20;30)
xs 1|echo
```

## Ranges And Slices

`a..b` stops before `b`. `a..=b` includes `b`. A middle point sets the stride.

```wq
1..5|echo
1..=5|echo
0..2..=10|echo
```

Ranges also slice lists.

```wq
xs:(10;20;30;40;50)
xs[1..4]|echo
xs[-3..=-1]|echo
```

## Mutating Lists

List indexes can be assigned.

```wq
xs:(10;20;30)
xs[1]:99
xs|echo
```

`[!]` pops from the end. `[!i]` removes at an index, and `[!i]:v` inserts there.

```wq
stack:()
stack,:10
stack,:20
stack[!]|echo
stack|echo
```

```wq
xs:(10;30)
xs[!1]:20
xs|echo
```

## Keep

- `(a;b;c)` is a list.
- `(a)` is just `a`; `,a` enlists one item.
- `xs[i]` indexes; `xs[a..b]` slices.
- Lists are mutable when you choose to mutate them.
