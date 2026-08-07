# Start Here

This book introduces the core wq language through small, runnable examples.
No prior wq experience is required.

In wqide, choose **Run** above a code block. Its output and final value appear
below it. Each standalone block starts a fresh session.

In the native CLI, open this book with `wq learn` and run inline code with
`wq exec 'CODE' -p`.

## Your First Result

Run this block. It returns `2`.

<!-- wq-example {"id":"first-result","expect":{"value":"2"}} -->
```wq
1+1
```

This result confirms that the evaluator is ready.

## Read the Result Panel

`echo` writes program output as ordinary lines. The final evaluated value starts
with a left-block prompt:

```text
hello from wq
▍ 42
```

The `▍` marks the final value in wqide. The native CLI prints the value without
this presentation marker when you pass `-p`.

## How Examples Work

A block can print intermediate output with `echo` and still return a final
value:

```wq
"hello from wq"|echo
6*7
```

The printed line is `hello from wq`; `▍ 42` is the final value. Expected-error
examples display a diagnostic instead.

## Run Related Cells Together

A cell group shares one fresh session. Choose **Run 2 cells** once:

<!-- wq-example {"id":"cell-group-values","cellGroup":"first-cell-group","expect":{"value":"(1;2;3)"}} -->
```wq
numbers:1..=3
```

<!-- wq-example {"id":"cell-group-sum","cellGroup":"first-cell-group","expect":{"value":"14"}} -->
```wq
numbers^2|sum
```

wqide runs the cells from top to bottom, so the second cell can use `numbers`
from the first. Each cell displays its own final value.

If a cell ends with `echo`, the printed line is followed by `▍ ()` because
`echo` returns `()`. Leave the inspected value as the final expression. Use
`echo` for intermediate output.

## Find Precise Help

Use the reference for exact language contracts:

```text
wq help calls
wq help chars-and-strings
wq help map
```

In wqide, open **Reference Docs** or use search.

## What You Will Build

The core path covers values, arithmetic, functions, containers, control flow,
errors, modules and a complete prime sieve. The optional final chapter covers
symbolic mathematics.

## Summary

- Standalone blocks start fresh. Grouped cells share one fresh session.
- `▍` marks the final evaluated value in wqide.
- Read the expected result before running an example.
- Use `wq help TOPIC` for the exact contract.
- Continue to **Values and Display**.
