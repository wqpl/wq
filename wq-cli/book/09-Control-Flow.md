# Control Flow

Control flow in wq is expression-shaped. Branches and loops produce values,
and blocks can appear inside other expressions.

Conditions must be bools. wq does not treat ints, strings, lists or other
values as truthy or falsey.

## Choose One

`$[condition;true;false]` is the basic conditional.

```wq
n:7
$[n%2=0;"even";"odd"]
```

The condition is first, then the true branch, then the false branch.

The false branch accepts more than one expression:
`$[condition;true;else1;else2...]`. Those expressions run only when the
condition is false, and the last one is the result.

```wq
n:7
$[n%2=0;"even";label:"odd";label]
```

With no false branch, a false condition yields an empty list.

```wq
$[F;"not reached"]
```

## Run When True

`$.[condition;body1;body2...]` runs its body only when the condition is true.
If the condition is false, the body is skipped and the result is an empty list.

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

That reads as `if grade>=90 then "A", else if grade>=80 then "B", else if
grade>=70 then "C", else "D"`.

The default is optional. If no condition matches and no default is present, the
result is an empty list.

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

This is often handy for building a list step by step.

`N` returns the last body result. A non-running `W` loop returns `()`.

## Repeat While True

`W[condition;body]` keeps going while the condition is true.

```wq
i:0
W[i<3;i+:1]
i
```

Use it when the stopping point is discovered while the program runs.

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

The second call uses brackets because an ungrouped leading minus after a name
would parse as subtraction.

## Keep

- `$[c;t;f]` chooses between two values.
- `$[c;t;f1;f2...]` runs a multi-expression false branch.
- `$.[c;t1;t2...]` runs a multi-expression body only when `c` is true.
- `$$[c1;t1;c2;t2;default]` checks cases in order.
- `N[n;body]` repeats with `_n`.
- `W[c;body]` repeats while `c` stays true.
- `@r` returns from a function.
- Continue to **Errors** to handle failed requirements deliberately.
