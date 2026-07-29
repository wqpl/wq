# Values and Display

wq programs move values through operators and functions. Values are either
atoms, such as an int or bool, or containers, such as a list or dict.

## Common Categories

| Category | Example | What it represents |
| --- | --- | --- |
| `int` | `42` | An exact whole number |
| `float` | `3.5` | A floating-point number |
| `fraction` | `7/.2` | An exact rational number |
| `bool` | `T` or `F` | A condition result |
| `char` | `"a"` | One Unicode scalar |
| `tag` | `` `name `` | A symbolic name |
| `list` | `(1;2;3)` | An ordered container |
| `dict` | ``(`name:"Ada";`age:3)`` | An ordered keyed container |

`type[value]` reports the public category:

```wq
(type[42];type[3.5];type[T];type[(1;2;3)])
```

The expected output is `("int";"float";"bool";"list")`.

## Chars and Strings

A quoted literal that decodes to one Unicode scalar is a char. Longer quoted
literals are strings.

```wq
(type["a"];type["Ada"])
```

The expected output is `("char";"list")`. Strings belong to the public `list`
category.

Use leading comma for a one-character string:

```wq
("a";,"a";type["a"];type[,"a"])
```

The char displays as `"a"`. The one-character string displays as `,"a"`.

`""` is an empty string in source. It displays as `()`, the same compact
display as an empty list. Use `type` or the surrounding operation when the
distinction matters.

## Bools Are Exact

Comparisons produce `T` or `F`:

```wq
(2<3;2=3)
```

wq has no truthy or falsey values. Branches and loops require a bool condition.

<!-- wq-example {"id":"bool-condition","expect":{"error":"domain"}} -->
```wq
$[1;"yes";"no"]
```

The expected diagnostic says `expected bool` and identifies `1` as an int.

## Tags

Tags begin with a backtick and usually name dict keys or named arguments:

```wq
(`ready;`name;`x2)
```

Tags are values. They are not strings and they do not look up bindings.

## Keep

- Atoms are non-container values; lists and dicts are containers.
- A one-scalar quoted literal is a char; longer quoted literals are strings.
- `T` and `F` are bools, and control flow accepts only bool conditions.
- `type[value]` reports the public category.
