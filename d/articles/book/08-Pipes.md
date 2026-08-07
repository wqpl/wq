# Pipes

Pipes pass a value from left to right.

```wq
1..=5|map{x*x}|sum
```

This expression creates `1..=5`, squares each item and sums the result.

Use `|` with data-first builtins.

## First Argument

`x|f[y]` inserts `x` as the first argument.

```wq
10|/[2]
```

That behaves like `/[10;2]`, so the result is `5.0`.

## Last Argument

`x||f[y]` inserts `x` as the last argument.

```wq
10||/[2]
```

That behaves like `/[2;10]`, so the result is `0.2`.

Argument position changes asymmetric operations such as division and
subtraction.

## Tap Pipes

`|.` and `||.` run the right-hand stage and pass the original value onward.

```wq
1..=3|.echo|sum
```

`echo` receives the range. The same range then flows into `sum`.

`|.` inserts the value first in the tap call. `||.` inserts it last.

## Checkpoints

A pipe stage can bind the value flowing through it.

<!-- wq-example {"id":"pipe-checkpoint-result","cellGroup":"pipe-checkpoint"} -->
```wq
1..=5|xs:|map{x*x}|sum
```

<!-- wq-example {"id":"pipe-checkpoint-value","cellGroup":"pipe-checkpoint"} -->
```wq
xs
```

`xs:` captures the range and passes it along unchanged.

## Pipes And Parentheses

Pipes bind more loosely than arithmetic.

```wq
1+2|*[10]
```

The addition runs first, so `3` flows into `*[10]`. Parentheses override this
order.

## Summary

- `x|f[y]` puts `x` first, as in `f[x;y]`.
- `x||f[y]` puts `x` last, as in `f[y;x]`.
- Use `|` with data-first builtins.
- `x|.f[y]` and `x||.f[y]` run a tap stage and pass `x` onward.
- `x|name:` binds a checkpoint.
- Pipes express transformation order from left to right.
- Continue to **Control Flow** when a pipeline needs a branch or loop.
