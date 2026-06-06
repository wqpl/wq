# Bindings

A **binding** associates a value with a name so you can reuse it later.

## Grammar

Bind a value to a name using a colon `:`, then use that name anywhere an expression fits:

```wq
answer:42
echo answer
```

```wq
greeting:"hello"
echo greeting
```

```wq
pair:(1;2)
pair+(10;10) |echo
```

## Naming rules

Names are case‑sensitive: `Total` and `total` are different.

You can use letters, digits, underscores, and question marks: `score`, `max_speed`, `a2`, `correct?`

However, there are a few restrictions:

- Don't start with a digit: `2bad`
- Don't start with a question mark: `?a`
- Built-in functions (bfns) like `echo` are reserved.
  - It means you can't use them for your own bindings.
  - In a native build of wq, enter `!bfn` in the REPL to see the list of these reserved names.

## Rebinding

Notice how wq allows you to change a binding's data type when replacing its value:

Hint: `type` is a bfn that returns a value's data type.

```wq
x:10
echo x
echo type x
echo "---"

x:x+1
echo x
echo type x
echo "---"

x:"oh hi!"
echo x
echo type x
```

## Binding is an expression

wq treats everything as an expression, meaning everything evaluates to a value.

Just as `1+2` evaluates to `3`, the binding `a:1` not only assigns the value but also evaluates to `1`.

```wq
echo(a:1)
```

## Chaining bindings

You can chain bindings because `:` is right-associative.

```wq
a:b:1  // same as a:(b:1)
echo a
echo b
```

`b:1` evaluates first, binding `1` to `b` and returning `1`.
Then, `a` is bound to that resulting `1`.

## Summary

- `name:value` binds a value to a name.
- Rebinding is allowed even if the new value has a different type.
- Bindings are expressions, meaning they produce a value.
