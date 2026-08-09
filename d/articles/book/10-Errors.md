# Errors

wq errors identify the failed operation, source location and value that
violated a requirement.

This example produces a `zero-div` error saying `cannot divide by zero`.

<!-- wq-example {"id":"error-zero-div","expect":{"error":"zero-div"}} -->
```wq
1/0
```

## Raise Your Own

Use `raise` when a condition makes the rest of the computation meaningless.

The expected error kind is `raise`, and its message is `nope`.

<!-- wq-example {"id":"error-raise","expect":{"error":"raise"}} -->
```wq
raise "nope"
```

Use `raise` to enforce a function-specific requirement.

<!-- wq-example {"id":"error-empty-head","expect":{"error":"raise"}} -->
```wq
head:{[xs]$.[#xs=0;raise "empty list"];xs 0}
head ()
```

## Assertions

`assert` requires a bool condition to be true.

```wq
assert[2<3]
"still here"
```

A false assertion stops execution with an `assert` error. A message and named
context add detail.

The expected error kind is `assert`.

<!-- wq-example {"id":"error-assert","expect":{"error":"assert"}} -->
```wq
assert[2>3;"ordering invariant failed";`context:`example]
```

Use `assert_eq` to compare whole values. It returns the actual value when the
comparison succeeds.

```wq
assert_eq[(1;2);(1;2)]
```

The expected output is `(1;2)`.

On failure, the error's `data` dict stores the check type, actual value,
expected value and optional context. Terminal notes display short excerpts.

## Try

`@t expr` returns the value or error as a tagged pair.

<!-- wq-example {"id":"try-success","cellGroup":"try-results"} -->
```wq
(@t 1+1)
```

<!-- wq-example {"id":"try-zero-div","cellGroup":"try-results"} -->
```wq
(@t 1/0)
```

<!-- wq-example {"id":"try-raised","cellGroup":"try-results"} -->
```wq
(@t raise "boom")
```

Success has the shape ``(`ok;value)``. Failure has the shape
``(`error;error_dict)``. The first item identifies the payload.

```wq
result:@t 1+1
$[result 0=`ok;result 1;raise "unexpected failure"]
```

The error dict has stable fields:

| Field | Use |
| --- | --- |
| `kind` | Stable tag for programmatic branching |
| `message` and `notes` | Explanation for people |
| `source` and `span` | Operation and source location |
| `data` | Structured details specific to the error |
| `stack` and `cause` | Calling context and nested failure |
| `version` | Structured error schema version |

Branch on the `kind` tag instead of parsing message strings.

## Summary

- Errors stop the current run.
- `raise` creates an error intentionally.
- `assert` checks a bool condition and `assert_eq` compares whole values.
- `@t` catches failure and returns a tagged result with the value or error.
- Continue to **Debugging with wqdb** to inspect a running program one step at
  a time.
