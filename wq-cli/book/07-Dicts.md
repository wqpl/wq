# Dicts

A dict stores named pieces of data. Dict keys are tags, written with a leading
backtick.

```wq
cat:(`name:"Ada";`age:3)
cat
```

The shape is still familiar: parentheses, semicolons, values. The tags make the
values addressable by name.

## Tag Names

Bare tag names follow the same identifier character rules as bindings.
`` `ready? ``, `` `λ2 `` and `` `猫 `` are valid tags. wq normalizes tag names
to Unicode NFC.

`tag[string]` converts a valid string to a tag. Tags are values, so wq syntax
and builtin names such as `` `T `` and `` `echo `` are valid tags.

## Reading Fields

Index a dict with a tag.

<!-- wq-example {"id":"dict-field-name","cellGroup":"dict-fields"} -->
```wq
cat:(`name:"Ada";`age:3)
cat`name
```

<!-- wq-example {"id":"dict-field-age","cellGroup":"dict-fields"} -->
```wq
cat`age
```

Bracket indexing works too:

```wq
cat:(`name:"Ada";`age:3)
cat[`name]
```

## Unpacking Fields

A tag-shaped assignment target selects fields by key. A shorthand tag creates a
binding with the same name, while `key:name` chooses a different target name.

```wq
cat:(`name:"Ada";`age:3)
(`name;`age:years):cat
(name;years)
```

When a key name is unavailable as a binding, choose another target name, as in
``(`T:truth):record``.

List and dict unpacking evaluate the source once and validate every requested
path before writing any target. A missing position or key leaves all pattern
targets unchanged.

## Stored Order

Dicts retain their entry order. Integer indexing reads values at zero-based positions, and negative positions count from the end.

<!-- wq-example {"id":"dict-order-first","cellGroup":"dict-order"} -->
```wq
rgb:(`r:80;`g:120;`b:200)
rgb 0
```

<!-- wq-example {"id":"dict-order-last","cellGroup":"dict-order"} -->
```wq
rgb[-1]
```

Order is part of a dict value. Dict equality and `hash` therefore distinguish
the same entries stored in different orders.

```wq
(`a:1;`b:2)=(`b:2;`a:1)
```

## Updating Fields

Dict fields can be updated like list indexes.

```wq
cat:(`name:"Ada";`age:3)
cat`age+:1
cat
```

That makes dicts a comfortable home for small records.

```wq
rgb:(`r:80;`g:120;`b:200)
rgb`g:160
rgb
```

## Keys and Values

`keys` and `values` return aligned lists in stored order.

<!-- wq-example {"id":"dict-keys","cellGroup":"dict-projections"} -->
```wq
rgb:(`r:80;`g:120;`b:200)
keys rgb
```

<!-- wq-example {"id":"dict-values","cellGroup":"dict-projections"} -->
```wq
values rgb
```

`list rgb` returns an ordered list of `(key;value)` pairs. Passing that list to
`dict` reconstructs the dict.

`dict` accepts only pairs whose first item is a tag. Convert a string key
explicitly with `tag` before building the pair.

## Transforming Dicts

Generic transforms operate on values. `map` and `filter` retain the associated
keys, while `sort` reorders whole entries by value. Use the named `by` option to
sort by key.

<!-- wq-example {"id":"dict-map","cellGroup":"dict-transforms"} -->
```wq
rgb:(`r:80;`g:120;`b:200)
map[rgb;{x+10}]
```

<!-- wq-example {"id":"dict-filter","cellGroup":"dict-transforms"} -->
```wq
filter[rgb;{x>=100}]
```

<!-- wq-example {"id":"dict-sort","cellGroup":"dict-transforms"} -->
```wq
sort[rgb;`by:`key]
```

Set algebra does not guess whether a dict means its keys or values. Project it first.

<!-- wq-example {"id":"dict-unique-keys","cellGroup":"dict-set-projections"} -->
```wq
rgb:(`r:80;`g:120;`b:200)
unique keys rgb
```

<!-- wq-example {"id":"dict-unique-values","cellGroup":"dict-set-projections"} -->
```wq
unique values rgb
```

## Tags Also Name Arguments

The same tag syntax appears at call sites.

<!-- wq-example {"id":"dict-named-argument","cellGroup":"dict-call-tags"} -->
```wq
scale:{[x;`by:2]x*by}
scale[10;`by:3]
```

<!-- wq-example {"id":"dict-default-argument","cellGroup":"dict-call-tags"} -->
```wq
scale 10
```

Here `by` is a named parameter with a default.

## Keep

- Dicts look like ``(`key:value;`other:value)``.
- Read with ``d`key`` or ``d[`key]``.
- Read by position with `d 0` or `d[-1]`.
- Update with forms like ``d`key+:1``.
- Unpack selected fields with a tag-shaped assignment target.
- Use `keys` and `values` to project stored entries explicitly.
- Tags also name arguments inside bracketed call argument lists.
- Continue to **Pipes** to transform values from left to right.
