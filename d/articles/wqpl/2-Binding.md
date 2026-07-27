# Binding

Binding is how a value gets a name.

```wq
steps:40+2
steps|echo
```

Read `steps:40+2` as "bind `40+2` to `steps`". A single `=` is equality; a colon is assignment.

## Names Are Reusable

Once a name is bound, it can appear anywhere an expression can appear.

```wq
radius:5
area:3.14159*radius^2
area|echo
```

You can rebind a name when the story changes:

```wq
mood:"curious"
mood|echo
mood:"very curious"
mood|echo
```

## Updating In Place

Operator-colon forms update from the old value.

```wq
n:10
n+:5
n*:2
n|echo
```

That same idea works with concatenation:

```wq
trail:()
trail,:10
trail,:20
trail|echo
```

## Assignment Has A Value

Binding is still an expression. It returns the value it assigned.

```wq
echo(total:sum(1;2;3))
total|echo
```

This can be handy in tiny examples, but use it gently. Code is allowed to breathe.

## Unpack A Shape

If the left side has a list shape, wq unpacks the right side into that shape.

```wq
point:(10;20)
(x;y):point
x+y|echo
```

Patterns may nest:

```wq
(name;(r;g;b)):("sky";(90;140;255))
name|echo
(r;g;b)|echo
```

`...` skips the middle when you only care about the ends.

```wq
(head;...;tail):(1;2;3;4)
(head;tail)|echo
```

## Unpack Dict Keys

A tag-shaped pattern selects dict entries by key, independent of their order.
A bare tag binds a same-named local:

```wq
(`x;`y):(`y:20;`x:10)
x+y|echo
```

Use `` `key:target `` to choose a different binding name:

```wq
(`width:w;`height:h):(`height:720;`width:1280)
(w;h)|echo
```

List and dict patterns can nest:

```wq
module:(`point:(10;20);`meta:(`version:1;`name:"plot"))
(`meta:(`name;`version);`point:(x;y)):module
(name;version;x;y)|echo
```

The right side runs once. wq reads every requested key before writing any
target, so a missing key raises an index error without partially updating the
pattern's bindings. Unmentioned dict keys are ignored. Duplicate requested
keys are a syntax error. `...` belongs to positional list patterns and is not
needed in a dict pattern.

## Pipe Checkpoints

A pipe can bind the value moving through it.

```wq
3*7|answer:
answer|echo
```

This is useful when a pipeline has a nice middle result you want to name without breaking the flow.

## Keep

- `name:value` binds.
- `name+:value` and friends update.
- `(a;b):value` unpacks a list by position.
- ``(`a;`b:alias):value`` unpacks a dict by key.
- `value|name:` names a value inside a pipeline.
