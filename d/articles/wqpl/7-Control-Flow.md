# Control Flow

Control flow in wq is still expression-shaped. Branches and loops produce values, and blocks can be tucked inside other expressions.

## Choose One

`$[condition;true;false]` is the basic conditional.

```wq
n:7
$[n%2=0;"even";"odd"]|echo
```

The condition is first, then the true branch, then the false branch.

The false branch may contain more than one expression: `$[condition;true;else1;else2...]`.
Those expressions run only when the condition is false, and the last one is the result.

```wq
n:7
$[n%2=0;"even";label:"odd";label]|echo
```

With no false branch, a false condition yields unit.

```wq
$[F;"not reached"]|echo
```

## Run When True

`$.[condition;body1;body2...]` runs its body only when the condition is true.
If the condition is false, the body is skipped and the result is unit.

```wq
x:0
$.[x=0;x:10;x+:5]
x|echo
```

## Choose From Several

`$$[...]` checks condition/action pairs in order. A final unpaired expression is the default.

```wq
grade:82
$$[grade>=90;"A";grade>=80;"B";grade>=70;"C";"D"]|echo
```

That reads as `if grade>=90 then "A", else if grade>=80 then "B", else if grade>=70 then "C", else "D"`.

The default is optional. If no condition matches and no default is present, the result is unit.

```wq
$$[F;"no";F;"also no"]|echo
```

## Repeat A Fixed Count

`N[count;body]` repeats the body. `_n` is the zero-based counter.

```wq
out:()
N[5;out,:_n]
out|echo
```

This is often handy for building a list step by step.

## Repeat While True

`W[condition;body]` keeps going while the condition is true.

```wq
i:0
W[i<3;i+:1]
i|echo
```

Use it when the stopping point is discovered while the program runs.

## Return Early

`@r` returns from the current function.

```wq
sign:{[x]$.[x=0;@r "zero"];$[x>0;"positive";"negative"]}
sign 0|echo
sign -5|echo
```

## Keep

- `$[c;t;f]` chooses between two values.
- `$[c;t;f1;f2...]` runs a multi-expression false branch.
- `$.[c;t1;t2...]` runs a multi-expression body only when `c` is true.
- `$$[c1;t1;c2;t2;default]` checks cases in order.
- `N[n;body]` repeats with `_n`.
- `W[c;body]` repeats while `c` stays true.
- `@r` returns from a function.
