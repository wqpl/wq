# Errors

When something goes wrong, wq raises an **error** which crashes your program.

Common causes include invalid math operations, out-of-bounds indexing, or calling a function with an incorrect number of arguments.

## Encountering errors

Try the run button:

```wq
1/0
```

```wq
a
```

```wq
ln[1;2]
```

Once an error is raised, evaluation stops unless you catch it with `@t`.

## Try `@t`

`@t expr` evaluates `expr` and converts success or failure to a regular value.

```wq
buggy:{1/0}
@t buggy[]
```

The result is a 2-element list:

- Elem 0:
  - the successful value, or
  - a list containing info about the error
- Elem 1:
  - on failure, a non-zero error code;
  - on success, `0`

```wq
buggy:{1/0}
(value;code):@t buggy[]
$[code=0;echo("ok: ",value);
  echo("failed: ",value 0)]
echo "---"
behaving:{1/1}
@t behaving[]
```

This is useful when you want to recover, report, or branch on a failure instead of crashing immediately.

## Summary

- `@t expr` turns evaluation into data: `(value_or_message; code)`.
- `code=0` means success; non-zero means error.
