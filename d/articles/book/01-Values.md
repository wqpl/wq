# Values and Display

wq programs move values through operators and functions. Non-container values
are atoms. Lists and dicts are containers.

## Value Categories

Every value belongs to one of the categories below. `type[value]` returns its
category name.

### Numbers

| Category | Example | What it represents |
| --- | --- | --- |
| `int` | `42` | An exact whole number |
| `fraction` | `7/.2` | An exact rational number |
| `float` | `3.5` | A binary floating-point number |
| `complex` | `3+4i` | A floating-point number with real and imaginary parts |
| `algebraic` | `@s root[t^2-2;t;1;2]` | An exact real algebraic number |

### Data and Containers

| Category | Example | What it represents |
| --- | --- | --- |
| `bool` | `T` or `F` | A true or false value |
| `char` | `"a"` | One Unicode scalar |
| `tag` | `` `name `` | A name stored as a value |
| `list` | `(1;2;3)` | An ordered container, including strings |
| `dict` | ``(`name:"Ada";`age:3)`` | An ordered container with tag keys |

### More Categories

| Category | Example | What it represents |
| --- | --- | --- |
| `function` | `{x+1}` | A callable function or builtin |
| `cas` | `@s x+1` | An expression for computer algebra |
| `rng` | `rng[42]` | A stateful random generator |
| `stream` | `open["data.bin"]` | An open byte stream |

Later chapters and reference pages explain the syntax used by these categories.

<!-- wq-example {"id":"category-int","cellGroup":"value-categories","expect":{"value":"\"int\""}} -->
```wq
type[42]
```

<!-- wq-example {"id":"category-float","cellGroup":"value-categories","expect":{"value":"\"float\""}} -->
```wq
type[3.5]
```

<!-- wq-example {"id":"category-bool","cellGroup":"value-categories","expect":{"value":"\"bool\""}} -->
```wq
type[T]
```

<!-- wq-example {"id":"category-list","cellGroup":"value-categories","expect":{"value":"\"list\""}} -->
```wq
type[(1;2;3)]
```

## Chars and Strings

A quoted literal that decodes to exactly one Unicode scalar is a char. A quoted
literal with zero or more than one Unicode scalar is a string.

<!-- wq-example {"id":"category-char","cellGroup":"char-string-categories","expect":{"value":"\"char\""}} -->
```wq
type["a"]
```

<!-- wq-example {"id":"category-string","cellGroup":"char-string-categories","expect":{"value":"\"list\""}} -->
```wq
type["Ada"]
```

Strings have category `list`.

Use leading comma for a one-character string:

<!-- wq-example {"id":"one-scalar-char","cellGroup":"one-character-string","expect":{"value":"\"a\""}} -->
```wq
"a"
```

<!-- wq-example {"id":"one-character-string","cellGroup":"one-character-string","expect":{"value":",\"a\""}} -->
```wq
,"a"
```

The char displays as `"a"`. The one-character string displays as `,"a"`.

`""` writes an empty string, while `()` writes an empty list. Both display as
`()` and have category `list`.

## Bool Conditions

Comparisons produce `T` or `F`:

<!-- wq-example {"id":"bool-less","cellGroup":"bool-comparisons","expect":{"value":"T"}} -->
```wq
2<3
```

<!-- wq-example {"id":"bool-equal","cellGroup":"bool-comparisons","expect":{"value":"F"}} -->
```wq
2=3
```

Branches and loops accept bool conditions only.

<!-- wq-example {"id":"bool-condition","expect":{"error":"domain"}} -->
```wq
$[1;"yes";"no"]
```

## Tags

Tags begin with a backtick. They commonly name dict keys and named arguments:

```wq
(`ready;`name;`x2)
```

A tag carries its name as a value. Writing `name` reads a binding, while writing
`` `name `` creates a tag without looking up a binding.

## Summary

- Atoms are non-container values. Lists and dicts are containers.
- A one-scalar quoted literal is a char. Other quoted literals are strings.
- `T` and `F` are bools. Control flow accepts bool conditions only.
- `type[value]` reports the public category.
