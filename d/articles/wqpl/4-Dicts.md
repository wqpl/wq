# Dicts

A dict stores named pieces of data. Dict keys are tags, written with a leading backtick.

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
- Use `keys` and `values` to project stored entries explicitly.
- Tags are also used for named arguments.
