# Lists

## Quick review

- A **list** is an ordered collection of values.
- Use parentheses `()` and separate items with semicolons `;`.
- A string is essentially a list of chars.
- Concatenation vs elementwise math
  - `,` concatenates lists.
  - `+` adds elementwise through broadcasting rules.

```wq
echo (1;2;3)
echo (1;"hi";true)
echo ((1;2);(3;4))
echo type "cat"
```

## The length operator `#`

`#x` gives you the length of a value.

```wq
xs:(10;20;30)
echo #xs
echo #"hello"
echo #()
```

The length of an _atom_ (a scalar) is 1.

```wq
echo #1
echo #true
```

## The unit `()`

The empty list, a.k.a. the unit, is written as `()`.

In practice, it is used as the "unit" value: a value that means "nothing useful to return". A function that only has side effects, like `echo`, returns a unit.

Hint: `unit?` checks if a value is of length 0.

```wq
fn:echo "hello"
echo fn
echo unit? fn
echo #fn
fn
```

## Indexing

Index with `[]` after a list.
Indexes are zero-based.

```wq
xs:(10;20;30)
echo xs[0]
echo xs[1]
```

Negative indexes count from the end:

```wq
xs:(10;20;30)
echo xs[-1]
echo xs[-2]
```

You can also select several positions at once:

```wq
xs:(10;20;30;40;50)
echo xs[-5..=-1]
```

## Mutating a list by index

Lists are mutable through indexed assignment:

```wq
xs:(10;20;30)
xs[1]:99
echo xs
```

## Ranges

The range builder `..` creates integer lists quickly.

```wq
1..5  |echo
1..10 |echo
```

`a..b` is half-open.
It includes `a` and stops before `b`.

For an inclusive end, use `..=`:

```wq
1..=5
```

You can also provide a step with an additional `..` at the end:

```wq
1..=11..2
```

Ranges are useful both on their own and inside indexing expressions.

## Formatted strings

Often you will want to build strings by injecting variables or expressions into them.
The `@f` prefix creates a formatted string. Enclose expressions inside curly braces `{}`.

```wq
a:9
echo @f"{a} plus 1 equals {a+1}"
```

## Matmul `**`

The operator `**` performs matrix multiplication.

```wq
A:(1;4)
B:(3;5)
A*B   |echo
"---" |echo
A**B  |echo
```

## Destructuring

You can unpack a list directly into multiple bindings using parentheses on the left side of `:` :

```wq
point:(100;200)
(x;y):point
echo x+y
```

## Summary

- `(a;b;c)` creates a list; `()` is the empty list.
- `#` inspects list length.
- `xs[i]` retrieves the value at index `i`.
  - Index is zero-based.
  - Index can be negative.
  - Index can be a list of ints.
- `a..b` and `a..=b..step` builds integer ranges.
