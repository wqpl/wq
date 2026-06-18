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

## Keys Are Values

`keys` returns the keys in insertion order.

```wq
rgb:(`r:80;`g:120;`b:200)
keys rgb|echo
rgb[keys rgb]|echo
```

That second line uses the keys to pull the values back out.

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
- Read with `d`key` or `d[`key]`.
- Update with forms like `d`key+:1`.
- Tags are also used for named arguments.
