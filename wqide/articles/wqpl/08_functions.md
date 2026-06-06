# Functions

A **function** is a reusable unit of logic that can be defined once and executed multiple times.

In wq, functions are values, so you can bind them to names, pass them around, return them from other functions, and store them inside lists or dicts.

## Basic grammar

Create a function with braces `{}`:

```wq
twice:{x*2}
echo twice
echo type twice
```

The last evaluated expression is the result returned.

```wq
f:{a:x+1;b:a*10;b}
f 2
```

## Parameters

There are two common ways to receive arguments.

### Implicit parameters

If you omit the parameter list, wq uses implicit names:

- `x` for the first argument
- `y` for the second
- `z` for the third

```wq
add:{x+y}
echo add[3;4]

mix:{100*x+10*y+z}
echo mix[1;2;3]
```

This style is short and convenient for small functions.

### Explicit parameters

Use brackets `[]` right after the opening brace bracket when you want your own parameter names or more clarity:

```wq
area:{[width;height]width*height}
echo area

clamp:{[lo;hi;v]max[lo;min[hi;v]]}
echo clamp
```

## Calling a function

Call a function with arguments surrounded in brackets `[]`.

```wq
add:{[a;b]a+b}
echo add[20;22]
```

```wq
sayhello:{echo "hello"}
echo sayhello
echo "---"
echo sayhello[]
echo "---"
sayhello[]
```

If you are only passing one argument, you can use the postfix style, which is visually neater:

```wq
sq:{x*x}
sq 9
```

## Values as functions

In wq, the line between data and logic is intentionally blurred: every value can be "called" like a function.

Think of collections (lists and dicts) as "discrete functions" that map an index or a key to a specific value. Just as you can evaluate a function using brackets (`sq[9]`) or postfix (`sq 9`), you can look up data the exact same way.

This rule further applies to atoms (scalars). If you "call" a single number with the index `0` or `-1`, it simply returns itself.

```wq
// "Calling" a list (mapping an index to a value)
xs:(10;20;30)
echo xs[1]
echo xs 1
echo "---"
// "Calling" a dict (mapping a symbol to a value)
person:(`name:"Alex";`age:25)
echo person `name
echo "---"
// "Calling" an atom
val:42
echo val[0]
echo val(-1)
```

## Return `@r`

Normally, a function returns the result of its last expression.
Use `@r value` when you want to leave the function immediately. If you omit `value`, the function returns `()`.

```wq
f:{[x]
  $.[x=0;@r -1]
  2+1/x}
f 2 |echo
f 0 |echo
```

Utlizing early returns with `@r` is the recommended style in wq, rather than writing long, deeply nested control flows.

## Closures

A **closure** is a function that captures bindings from its surrounding environment.

This happens when you use a binding that hasn't been declared in the function's body but exists outside of it:

```wq
a:5
b:{a}
echo b[]
echo type b
```

```wq
make_adder:{[a]{x+a}}
add5:make_adder 5
add5 3
```

Here, `add5` remembers that `a` was `5`.

## Captures capture by value

wq closures capture bindings _by value_, not by "reference".
That means the remembered value does not change later.

```wq
a:3
f:{a}
a:4
f[]
```

The same rule applies to locals created inside another function:

```wq
outer:{
  a:4
  inner:{a}
  a:5
  inner}
outer[][]
```

This makes closures predictable, but it also means mutual recursion is not possible.

## Self-recursion

Functions can call themselves by name:

```wq
fact:{[n]$[n<=1;1;n*fact[n-1]]}
fact 5
```

```wq
fib:{fib_:{[n;a;b]$[n=0;a;fib_[n-1;b;a+b]]}
  fib_[x;0;1]}
fib 10
```

## Summary

- `{[param1;...]...}` creates a function.
- Omit `[param1;...]` for short functions.
- The last expression returns automatically; `@r` exits early.
- Closures remember captured values, and the captures are locked to the value at creation time.
