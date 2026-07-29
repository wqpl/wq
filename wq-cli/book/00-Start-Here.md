# Start Here

This book teaches the core wq language by letting you run small, complete
examples. You do not need prior wq experience.

In wqide, choose **Run** above a standalone wq code block. Printed output and
the final value appear directly below it. Every standalone block starts with a
fresh session.

In the native CLI, open this book with `wq learn` and run inline code with
`wq exec 'CODE' -p`.

## Your First Result

Run this block. The expected result is `2`.

<!-- wq-example {"id":"first-result","expect":{"value":"2"}} -->
```wq
1+1
```

If you see `2`, your environment is ready. If wqide shows an error before the
example runs, reload the page. In the CLI, run `wq --version` to confirm that
the command is installed.

## Read the Result Panel

`echo` writes program output as ordinary lines. The final evaluated value starts
with a left-block prompt:

```text
hello from wq
▍ 42
```

The `▍` distinguishes the final value from anything the program printed. It is
part of wqide presentation, not part of the wq value. The native CLI prints the
same final value without this marker when you pass `-p`.

## How Examples Work

A block can print intermediate output with `echo` and still return a final
value:

```wq
"hello from wq"|echo
6*7
```

The printed line is `hello from wq`, and `▍ 42` marks the final value.

Some examples intentionally fail so you can see a diagnostic. The page labels
those results as expected errors. They do not damage your session or files.

## Run Related Cells Together

A cell group packs related steps into one example. Choose **Run 2 cells** once:

<!-- wq-example {"id":"cell-group-values","cellGroup":"first-cell-group","expect":{"value":"(1;2;3)"}} -->
```wq
numbers:1..=3
```

<!-- wq-example {"id":"cell-group-sum","cellGroup":"first-cell-group","expect":{"value":"14"}} -->
```wq
numbers^2|sum
```

wqide runs the cells from top to bottom in one fresh session, so the second cell
can use `numbers` from the first. Each final value gets its own highlighted
result. This is why grouped examples do not need `echo` just to show an
intermediate value.

Every cell shows its actual final value. If a cell ends with `echo`, the printed
line is followed by `▍ ()` because `echo` returns `()`. Leave the value you want
to inspect as the final expression, and reserve `echo` for intermediate output.

## Find Precise Help

The book builds a working mental model. The reference gives exact details when
you need them:

```text
wq help calls
wq help chars-and-strings
wq help map
```

In wqide, open **Reference Docs** or use search.

## What You Will Build

The core path moves from values and arithmetic through functions, containers,
control flow, errors, modules and one complete prime-sieve program. The final
CAS chapter is optional and introduces symbolic mathematics.

## Keep

- Standalone blocks start fresh; grouped cells share one fresh session.
- Read `▍` as “final evaluated value,” not as part of the value.
- Read the expected result before running an example.
- Use `wq help TOPIC` when you need the exact contract.
- Continue to **Values and Display**.
