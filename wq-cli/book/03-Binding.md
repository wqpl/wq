# Binding

Binding is how a value gets a name.

```wq
steps:40+2
steps
```

Read `steps:40+2` as "bind `40+2` to `steps`". A single `=` is equality; a colon is assignment.

## Names

A name starts with `_` or a character with the Unicode `XID_Start` property,
such as `a`, `λ`, or `猫`. Later characters can be `_`, `?`, or characters with
the `XID_Continue` property, such as `2` or a combining accent.

```wq
λ2?:7
λ2?
```

`ready?` and `λ2?` are valid names. `?ready` and `1st` are not. wq normalizes
names to Unicode NFC, so canonically equivalent spellings refer to the same
binding.

Some identifier-shaped spellings are already wq syntax:

- `T`, `F`, and `inf` are literals.
- `W`, `N`, `B`, `A`, `and`, `O`, and `or` are language forms.

Those spellings cannot be binding names. A builtin-function name available in
the current builtin set cannot be rebound either.

## Names Are Reusable

Once a name is bound, it can appear anywhere an expression can appear.

```wq
radius:5
area:3.14159*radius^2
area
```

You can rebind a name when the story changes:

<!-- wq-example {"id":"binding-first-mood","cellGroup":"rebind-mood"} -->
```wq
mood:"curious"
```

<!-- wq-example {"id":"binding-second-mood","cellGroup":"rebind-mood"} -->
```wq
mood:"very curious"
```

## Updating In Place

Operator-colon forms update from the old value.

```wq
n:10
n+:5
n*:2
n
```

That same idea works with concatenation:

```wq
trail:()
trail,:10
trail,:20
trail
```

## Assignment Has A Value

Binding is still an expression. It returns the value it assigned.

<!-- wq-example {"id":"assignment-result","cellGroup":"assignment-value","expect":{"value":"6"}} -->
```wq
total:sum(1;2;3)
```

<!-- wq-example {"id":"assignment-reuse","cellGroup":"assignment-value","expect":{"value":"6"}} -->
```wq
total
```

This can be handy in tiny examples, but use it gently. Code is allowed to breathe.

## Keep

- `name:value` binds.
- `name+:value` and friends update.
- Continue to **Calls, Indexing, and Postfix** to apply functions and
  containers.
