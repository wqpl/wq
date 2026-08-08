# Control Flow

Control-flow forms are expressions. Branches and loops produce values, and
their conditions accept bools only.

## Combine Conditions

`not` reverses one bool:

<!-- wq-example {"id":"bool-not","expect":{"value":"F"}} -->
```wq
not[T]
```

`and[...]` returns `T` only when every condition is `T`. `or[...]` returns `T`
when at least one condition is `T`.

<!-- wq-example {"id":"bool-and","cellGroup":"short-circuit-bools","expect":{"value":"T"}} -->
```wq
and[3<5;5<10]
```

<!-- wq-example {"id":"bool-or","cellGroup":"short-circuit-bools","expect":{"value":"T"}} -->
```wq
or[3>5;5<10]
```

Both forms short-circuit from left to right. `and` stops at the first `F`, while
`or` stops at the first `T`; expressions after that point are not evaluated.
`A[...]` and `O[...]` are shorter spellings of `and[...]` and `or[...]`.

`band`, `bor` and `bxor` also accept bools, but they evaluate every argument.
Use `bxor` when exactly one of two bools should be true:

<!-- wq-example {"id":"bool-bitwise-and","cellGroup":"eager-bool-logic","expect":{"value":"F"}} -->
```wq
band[T;F]
```

<!-- wq-example {"id":"bool-bitwise-or","cellGroup":"eager-bool-logic","expect":{"value":"T"}} -->
```wq
bor[T;F]
```

<!-- wq-example {"id":"bool-bitwise-xor","cellGroup":"eager-bool-logic","expect":{"value":"T"}} -->
```wq
bxor[T;F]
```

The same three builtins apply bit by bit to ints, as shown in
[Arithmetic](02-Arithmetic.md).

## Choose One

`$[condition;true;false]` is the basic conditional.

```wq
n:7
$[n%2=0;"even";"odd"]
```

Arguments appear in condition, true branch and false branch order.

`$[condition;true;else1;else2...]` gives the false branch multiple
expressions. Its final expression becomes the result.

```wq
n:7
$[n%2=0;"even";label:"odd";label]
```

An omitted false branch defaults to an empty list.

```wq
$[F;"not reached"]
```

## Run When True

`$.[condition;body1;body2...]` runs its body for a true condition. A false
condition returns an empty list.

```wq
x:0
$.[x=0;x:10;x+:5]
x
```

Use a standalone bracket block when several expressions must act as one branch.
The block returns its last expression.

```wq
x:0
$.[x=0;[x:10;x+:5]]
x
```

## Choose From Several

`$$[...]` checks condition/action pairs in order. A final unpaired expression is
the default.

```wq
grade:82
$$[grade>=90;"A";grade>=80;"B";grade>=70;"C";"D"]
```

Conditions are checked from left to right. The first matching action supplies
the result.

The optional default is the final unpaired expression. Without a match or
default, the result is an empty list.

```wq
$$[F;"no";F;"also no"]
```

## Repeat A Fixed Count

`N[count;body]` repeats the body. `_n` is the zero-based counter.

```wq
out:()
N[5;out,:_n]
out
```

`N` returns the last body result. A non-running `W` loop returns `()`.

## Repeat While True

`W[condition;body]` keeps going while the condition is true.

```wq
i:0
W[i<3;i+:1]
i
```

The condition is checked before each iteration.

## Return Early

`@r` returns from the current function.

<!-- wq-example {"id":"control-return-zero","cellGroup":"return-early"} -->
```wq
sign:{[x]$.[x=0;@r "zero"];$[x>0;"positive";"negative"]}
sign 0
```

<!-- wq-example {"id":"control-return-negative","cellGroup":"return-early"} -->
```wq
sign[-5]
```

The second call brackets its negative argument because postfix form would parse
the leading minus as subtraction.

## Summary

- `not`, `and` and `or` combine bool conditions; `and` and `or` short-circuit.
- `band`, `bor` and `bxor` combine bools eagerly.
- `$[c;t;f]` chooses between two values.
- `$[c;t;f1;f2...]` runs a multi-expression false branch.
- `$.[c;t1;t2...]` runs a multi-expression body only when `c` is true.
- `$$[c1;t1;c2;t2;default]` checks cases in order.
- `N[n;body]` repeats with `_n`.
- `W[c;body]` repeats while `c` stays true.
- `@r` returns from a function.
- Continue to **Errors** to handle failed requirements deliberately.
