# Higher-order bfns

A **higher-order** function is one that either accepts other functions as arguments or returns them.

wq provides several higher-order bfns to help you write clean, concise, and data-oriented code.

## `filter`

`filter[xs;f]` keeps only the items in your list `xs` for which the given function `f` returns `true`.

```wq
xs:1..=10
filter[xs;{x%2=0}]
```

## `map`

`map[xs;f]` creates a new list by applying your function `f` to every item in your list `xs`.

```wq
xs:1..=10
map[xs;{x*10}]
```

## `fold`

`fold[xs;f;acc]` "folds" a list into a single value.

It starts with an initial value `acc` and processes your list `xs` item-by-item.

For each item, it uses your function `f` to combine the current result with that item to create a new, updated result.

```wq
xs:1..=5
fold[xs;{x+y};100]
```

You can omit `acc`. In that case, `fold[xs;f]`:

- Checks if `xs` is empty. If it is, it returns `()`.
- If not empty, it takes the first item as the initial accumulator.
- It then iterates over the _remaining_ items, applying your folding function `f`.

```wq
xs:1..=5
fold[xs;{x+y}]
```

## `scan`

`scan[xs;f;acc]` (and `scan[xs;f]`) is like `fold`, but instead of returning only the final result, it returns a list of all intermediate values.

```wq
xs:1..=5
scan[xs;{x+y}]
```

## Pipe `|`

Pipe syntax lets you write a chain of function applications from left to right. It passes the value on the left as the first argument to the function on the right.

```wq
iota 10 |sum |echo
```

The pipeline above is equivalent to:

```wq
echo sum iota 10
```

This is useful when several higher-order functions each consume the previous result, where in normal syntax nested parentheses would be necessary:

```wq
iota 10
|filter{x%2=0}
|map{x*x}
|sum
|echo
```

## Summary

- `filter[xs;f]` keeps elements matching a condition.
- `map[xs;f]` transforms every element.
- `fold[xs;f]` reduces elements to a single value.
- `scan[xs;f]` yields all intermediate reduction steps.
- `|` pipes a value through a chain of function calls, injecting it as the first argument.
