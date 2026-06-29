# Pipes

Pipes let a value move left to right.

```wq
1..=5|map{x*x}|sum|echo
```

Read it as: make `1..=5`, square each item, sum the result, show it.

Most pipelines use `|`. wq builtins are generally designed with the main data as the first argument, so the ordinary pipe is the default shape for daily code.

## First Argument

`x|f[y]` inserts `x` as the first argument. Use it when the flowing value belongs on the left side of an asymmetrical operation.

```wq
10|/[2]|echo
```

That behaves like `/[10;2]`, so the result is `5.0`.

## Last Argument

`||` inserts the value as the last argument. Use it when the flowing value belongs on the right side or at the end of the call.

```wq
10||/[2]|echo
```

That behaves like `/[2;10]`, so the result is `0.2`.

For symmetric calls, first or last may not matter much. For asymmetrical calls like divide and subtract, choose the side deliberately.

## Tap Pipes

`|.` and `||.` run the right-hand stage but keep the original value flowing.

```wq
1..=3|.echo|map{x}|sum|echo
```

The `echo` stage sees the range, but the range keeps flowing into the next stage.

`|.` inserts the value as the first argument to the tap stage. `||.` inserts it as the last argument. Use `||.` for the same reason as `||`: the tap call expects the flowing value at the end.

## Checkpoints

A pipe stage can bind the value flowing through it.

```wq
1..=5|xs:|map{x*x}|sum|echo
xs|echo
```

`xs:` captures the range and passes it along unchanged.

## Pipes And Parentheses

Pipes bind loosely. That is usually what you want.

```wq
1+2|*[10]|echo
```

The addition happens before the pipe, so `3` flows into `*[10]`.

When in doubt, add parentheses. They are cheap and kind.

## Keep

- `x|f[y]` puts `x` first, as in `f[x;y]`.
- `x||f[y]` puts `x` last, as in `f[y;x]`.
- Most builtins are data-first, so `|` is the pipe you will use most.
- `x|.f[y]` and `x||.f[y]` run a tap stage and keep `x`.
- `x|name:` binds a checkpoint.
- Pipes make transformation chains read in order.
