# Pipes

Pipes let a value move left to right.

```wq
1..=5|map{x*x}|sum|echo
```

Read it as: make `1..=5`, square each item, sum the result, show it.

## First Argument

`x|f[y]` inserts `x` as the first argument.

```wq
10|-[3]|echo
```

That behaves like `-[10;3]`, so the result is `7`.

## Last Argument

`||` inserts the value as the last argument.

```wq
10||-[3]|echo
```

That behaves like `-[3;10]`, so the result is `-7`.

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

- `x|f[y]` puts `x` first.
- `x||f[y]` puts `x` last.
- `x|name:` binds a checkpoint.
- Pipes make transformation chains read in order.
