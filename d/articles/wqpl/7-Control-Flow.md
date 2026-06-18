# Control Flow

Control flow in wq is still expression-shaped. Branches and loops produce values, and blocks can be tucked inside other expressions.

## Choose One

`$[condition;true;false]` is the basic conditional.

```wq
n:7
$[n%2=0;"even";"odd"]|echo
```

The condition is first, then the true branch, then the false branch.

## Choose From Several

`$$[...]` checks condition/action pairs in order.

```wq
grade:82
$$[grade>=90;"A";grade>=80;"B";"C"]|echo
```

The final item is the fallback.

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

The dotted conditional `$.[condition;body]` runs the body only when the condition is true.

## Keep

- `$[c;t;f]` chooses between two values.
- `$$[...]` checks several cases.
- `N[n;body]` repeats with `_n`.
- `W[c;body]` repeats while `c` stays true.
- `@r` returns from a function.
