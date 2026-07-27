# Dicts

A dict stores named pieces of data. Dict keys are tags, written with a leading backtick.

Bare tag names follow identifier character rules. The first character is `_`
or has the Unicode `XID_Start` property, such as `a`, `λ`, or `猫`. Later
characters can be `_`, `?`, or have the `XID_Continue` property, such as `2`
or a combining accent.

For example, `` `ready? `` and `` `λ2 `` are valid tags; `` `?ready `` and
`` `1st `` are not. wq normalizes tag names to Unicode NFC. `tag[string]`
applies the same character and normalization rules. Tags are values, so wq
syntax and builtin names such as `` `T `` and `` `echo `` are valid tags.

```wq
cat:(`name:"Ada";`age:3)
cat|echo
```

The shape is still familiar: parentheses, semicolons, values. The tags make the values addressable by name.

## Reading Fields

Index a dict with a tag.

```wq
cat:(`name:"Ada";`age:3)
cat`name|echo
cat`age|echo
```

Bracket indexing works too:

```wq
cat:(`name:"Ada";`age:3)
cat[`name]|echo
```

## Unpacking Fields

A tag-shaped assignment target selects fields by key. A shorthand tag creates a binding with the same name, while `key:name` chooses a different target name.

```wq
cat:(`name:"Ada";`age:3)
(`name;`age:years):cat
(name;years)|echo
```

When a key name is unavailable as a binding, choose another target name, as in ``(`T:truth):record``.

List and dict unpacking evaluate the source once and validate every requested path before writing any target. A missing position or key leaves all pattern targets unchanged.

## Stored Order

Dicts retain their entry order. Integer indexing reads values at zero-based positions, and negative positions count from the end.

```wq
rgb:(`r:80;`g:120;`b:200)
rgb 0|echo
rgb[-1]|echo
```

Order is part of a dict value. Dict equality and `hash` therefore distinguish the same entries stored in different orders.

```wq
(`a:1;`b:2)=(`b:2;`a:1)|echo
```

## Updating Fields

Dict fields can be updated like list indexes.

```wq
cat:(`name:"Ada";`age:3)
cat`age+:1
cat|echo
```

That makes dicts a comfortable home for small records.

```wq
rgb:(`r:80;`g:120;`b:200)
rgb`g:160
rgb|echo
```

## Keys and Values

`keys` and `values` return aligned lists in stored order.

```wq
rgb:(`r:80;`g:120;`b:200)
keys rgb|echo
values rgb|echo
```

`list rgb` returns an ordered list of `(key;value)` pairs. Passing that list to `dict` reconstructs the dict.

`dict` accepts only pairs whose first item is a tag. Convert a string key explicitly with `tag` before building the pair.

## Transforming Dicts

Generic transforms operate on values. `map` and `filter` retain the associated keys, while `sort` reorders whole entries by value. Use the named `by` option to sort by key.

```wq
rgb:(`r:80;`g:120;`b:200)
map[rgb;{x+10}]|echo
filter[rgb;{x>=100}]|echo
sort[rgb;`by:`key]|echo
```

Set algebra does not guess whether a dict means its keys or values. Project it first.

```wq
rgb:(`r:80;`g:120;`b:200)
unique keys rgb|echo
unique values rgb|echo
```

## Tags Also Name Arguments

The same tag syntax appears at call sites.

```wq
scale:{[x;`by:2]x*by}
scale[10;`by:3]|echo
scale 10|echo
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
