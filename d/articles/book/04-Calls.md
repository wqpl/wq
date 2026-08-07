# Calls, Indexing, and Postfix

Functions and containers share one application shape. The left value determines
whether brackets call or index.

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

Use brackets for zero arguments, named arguments and multiple arguments.
Postfix form supplies exactly one argument.

## Group Negative Arguments

A leading minus after a name parses as subtraction. Group or bracket a negative
postfix argument.

```wq
sign:{[x]$[x<0;"negative";"non-negative"]}
(sign[-5];sign (-5))
```

Both calls return `"negative"`.

## Postfix Binds Tightly

Postfix binds more tightly than `+`, so `echo 1+2` parses as `(echo 1)+2` and
prints `1`. Group the argument expression:

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

## Summary

- `target[...]` calls a callable or indexes a container.
- Bracket entries are separated with semicolons.
- Postfix application has one argument slot.
- Group or bracket a negative postfix argument.
- Chained indexes form a deep path.
