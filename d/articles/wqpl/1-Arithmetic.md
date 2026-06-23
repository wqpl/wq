# Arithmetic

Arithmetic is the easiest place to feel wq's personality: small expressions, no ceremony, and lists that do math with you.

When a block ends with `|echo`, read it as "show the value on the left".

## Calculator Mode

The familiar operators are here. `^` means power.

```wq
1+2*3   |echo
(1+2)*3 |echo
2^8     |echo
```

Precedence is ordinary enough to trust for small expressions, and parentheses are there when you want the expression to say exactly what you mean.

Power groups from the right:

```wq
2^3^2   |echo
(2^3)^2 |echo
```

So the first line is `2^(3^2)`, not `(2^3)^2`.

## Division Has Flavors

`/` gives a floating result. `/%` gives floor division. `%` gives the remainder. `/.` keeps exact fractional results when it can.

```wq
7/2  |echo
7/%2 |echo
7%2  |echo
7/.2 |echo
```

Divide by zero and wq stops you where the bad expression happened:

```wq error
1/0
```

## Power Has Flavors Too

`^` is the everyday runtime power operator. Positive integer powers stay exact when they can, but negative or fractional numeric powers use classic floating-point arithmetic.

```wq
2^8           |echo
2^-3          |echo
(8/.27)^(1/.3)|echo
```

`^.` asks for exact exponentiation. Pair it with exact operands such as `/.` when the exponent is fractional:

```wq
2^.-3           |echo
(8/.27)^.(1/.3) |echo
```

## Lists Join In

A list is written with parentheses and semicolons:

```wq
(1;2;3)
```

The fun part is that arithmetic works through the shape of a value.

```wq
(1;2;3)+10            |echo
(1;2;3)*(10;100;1000) |echo
((1;2);(3;4))+(10;20) |echo
```

That is broadcasting: an atom can flow across a list, and matching lists combine item by item.

Mismatched shapes are not guessed at:

```wq error
(1;2;3)+(10;20)
```

## Plus Is Not Glue

`+` adds. The comma `,` concatenates.

```wq
(1;2),(3;4) |echo
(1;2)+(3;4) |echo
```

That distinction matters early. Once you trust it, list code becomes easier to read: math looks like math, joining looks like joining.

## Tiny Curves

Because arithmetic broadcasts, a list can stand in for a little row of inputs.

```wq
(0;1;2;3;4;5)^2 |echo
```

You can pipe the result into a bfn too. Here `sum` adds the squared values:

```wq
(1;2;3;4;5)^2|sum |echo
```

It is a small thing, but this is the shape of a lot of wq: make a value, transform it, pass it along.

## Number Questions

Comparisons produce booleans, `T` or `F`.

```wq
1<2<3     |echo
(1;2;3)>1 |echo
```

Plain `=` compares whole values. Dotted `=.` compares through matching leaves:

```wq
(1;2;3)=(1;9;3)  |echo
(1;2;3)=.(1;9;3) |echo
```

## Keep

- `+ - * / % ^` are the everyday arithmetic operators.
- `/%` is floor division; `/.` is exact division; `^.` is exact power.
- Arithmetic broadcasts over compatible list shapes.
- `,` concatenates; `+` never means concatenate.
