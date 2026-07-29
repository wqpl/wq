# Calls, Indexing, and Postfix

Functions and containers share one application shape. The value on the left
decides whether brackets mean a call or an index.

## Bracket Form

If the target is callable, brackets call it:

```wq
add:{[x;y]x+y}
add[2;3]
```

If the target is a list, string or dict, brackets index it:

```wq
xs:(10;20;30)
xs[1]
```

Semicolons separate call arguments. On a container, several bracket entries
select several positions at the current depth:

```wq
xs:(10;20;30;40)
xs[0;2]
```

The expected output is `(10;30)`.

## Postfix Form

Postfix application has exactly one argument slot:

```wq
sq:{x*x}
sq 9
```

For a container, the same form indexes one item:

```wq
xs:(10;20;30)
xs 1
```

Use brackets for zero arguments, named arguments or more than one argument.
`fn arg1 arg2` does not mean a two-argument call.

## Group Negative Arguments

A leading minus after a name parses as subtraction, not as a negative postfix
argument.

```wq
sign:{[x]$[x<0;"negative";"non-negative"]}
(sign[-5];sign (-5))
```

Both calls return `"negative"`. Do not write `sign -5`.

## Postfix Binds Tightly

`echo 1+2` parses as `(echo 1)+2`, so it prints `1` instead of `3`. Group an
expression before passing it:

```wq
echo (1+2)
echo[1+2]
```

Both lines print `3`.

## Deep Paths

Chained indexes descend one layer at a time:

```wq
xs:((1;2);(3;4))
(xs[1][0];xs[1] 0)
```

Both paths return `3`.

## Keep

- `target[...]` calls a callable or indexes a container.
- Bracket entries are separated with semicolons.
- Postfix application has one argument slot.
- Group or bracket a negative postfix argument.
- Chained indexes form a deep path.
