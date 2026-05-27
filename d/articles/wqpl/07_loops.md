# Loops

A **loop** let you run code repeatedly: for a fixed number of times, while a condition holds, or once per element in a value.

A loop is an expression, it evaluates to the result of its last iteration.
If a loop never runs, it yields `()`.

## Overview

wq has three forms of loop: N-loop, W-loop, and F-loop.

- `N[n;body]`
- `W[cond;body]`
- `F[value;body]`

## N-loop

The N-loop executes its body `n` times when `n>=0`.

- If `n<0`, it yields the unit value `()`.
- The counter is exposed as `_n`.
- `_n` starts at `0` and increases by `1` each iteration.
- Mutations to `_n` are reset at the start of each iteration.

```wq
N[3;echo _n]
```

```wq
N[0;echo _n]
```

```wq
N[-1;echo _n]
```

## W-loop

The W-loop repeatedly evaluates `cond` and runs the body while the condition remains true.

Unlike some other languages, `cond` must be a bool. wq does not coerce other values into truthy or falsy ones.

If the condition is false at the start, the body does not run, and the loop yields `()`.

```wq
i:0
echo W[rand[]<0.8;i:i+1]
```

```wq
W[false;echo ":)"]
```

## F-loop

The F-loop iterates over a value.

- The counter is exposed as `_n`.
- The current element is exposed as `_f`.

```wq
x:(10;20;30)
F[x;echo (_n;_f)]
```

```wq
x:1
F[x;echo (_n;_f)]
```

## Manipulating loops

Inside loops, `@c` skips to the next iteration, and `@b` breaks out of the current loop.

### Continue `@c`

```wq
n:0
N[5;$.[_n=2;@c];n:n+1]
echo n
```

When `_n=2`, the rest of that loop body is skipped, so `n` is incremented 4 times instead of 5.

### Break `@b`

```wq
acc:0
F[(1;2;3;4;5)
  $.[_f=4;@b]
  acc:acc+_f]
```

Once `_f=4`, the loop exits immediately.

Both `@b` and `@c` apply to the nearest enclosing loop.

## Summary

- `N[n;...]` repeats a fixed number of times and exposes `_n`.
- `W[cond;...]` repeats while a bool condition stays true.
- `F[value;...]` iterates over elements and exposes `_n` and `_f`.
- `@b` leaves the loop, and `@c` jumps to the next iteration.
