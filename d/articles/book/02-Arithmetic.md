# Arithmetic

wq works like a calculator for individual numbers. The same arithmetic also
follows the shape of compatible lists.

## Everyday Arithmetic

`+`, `-` and `*` add, subtract and multiply. `^` means power. `**` does not: it
performs matrix multiplication.

<!-- wq-example {"id":"calculator-precedence","cellGroup":"calculator-mode","expect":{"value":"7"}} -->
```wq
1+2*3
```

<!-- wq-example {"id":"calculator-parentheses","cellGroup":"calculator-mode","expect":{"value":"9"}} -->
```wq
(1+2)*3
```

<!-- wq-example {"id":"calculator-power","cellGroup":"calculator-mode","expect":{"value":"256"}} -->
```wq
2^8
```

Multiplication binds more tightly than addition. Parentheses set explicit
grouping. The Calls chapter covers the tighter precedence of postfix calls.

Power groups from the right:

<!-- wq-example {"id":"power-right","cellGroup":"power-grouping","expect":{"value":"512"}} -->
```wq
2^3^2
```

<!-- wq-example {"id":"power-left","cellGroup":"power-grouping","expect":{"value":"64"}} -->
```wq
(2^3)^2
```

The first expression means `2^(3^2)`. Parentheses make the second expression
mean `(2^3)^2`.

## Choose a Division Result

The division operator determines the kind of result:

| Operator | Meaning | `7` and `2` produce |
| --- | --- | --- |
| `/` | Floating division | `3.5` |
| `/%` | Floor division, rounded down | `3` |
| `%` | Remainder | `1` |
| `/.` | Exact division | `7/2` |

<!-- wq-example {"id":"division-float","cellGroup":"division-flavors","expect":{"value":"3.5"}} -->
```wq
7/2
```

<!-- wq-example {"id":"division-floor","cellGroup":"division-flavors","expect":{"value":"3"}} -->
```wq
7/%2
```

<!-- wq-example {"id":"division-remainder","cellGroup":"division-flavors","expect":{"value":"1"}} -->
```wq
7%2
```

<!-- wq-example {"id":"division-exact","cellGroup":"division-flavors","expect":{"value":"7/2"}} -->
```wq
7/.2
```

All four forms reject a zero divisor with a `zero-div` error.

<!-- wq-example {"id":"divide-by-zero","expect":{"error":"zero-div"}} -->
```wq
1/0
```

## Approximate and Exact Powers

`^` uses runtime numeric arithmetic. Positive int powers stay exact when
possible. Negative and fractional numeric powers can produce floats or complex
values.

<!-- wq-example {"id":"classic-power-negative","cellGroup":"classic-power","expect":{"value":"0.125"}} -->
```wq
2^-3
```

<!-- wq-example {"id":"classic-power-fractional","cellGroup":"classic-power","expect":{"value":"0.6666666666666666"}} -->
```wq
(8/.27)^(1/.3)
```

`^.` performs exact exponentiation. Use exact operands such as `/.` for an
exact fractional exponent:

<!-- wq-example {"id":"exact-power-negative","cellGroup":"exact-power","expect":{"value":"1/8"}} -->
```wq
2^.-3
```

<!-- wq-example {"id":"exact-power-fractional","cellGroup":"exact-power","expect":{"value":"2/3"}} -->
```wq
(8/.27)^.(1/.3)
```

## Broadcasting

With a list input, ordinary arithmetic follows the list's shape. An atom is
reused for each item, while two lists line up by position.

<!-- wq-example {"id":"broadcast-atom","cellGroup":"broadcasting","expect":{"value":"(11;12;13)"}} -->
```wq
(1;2;3)+10
```

<!-- wq-example {"id":"broadcast-matching","cellGroup":"broadcasting","expect":{"value":"(10;200;3000)"}} -->
```wq
(1;2;3)*(10;100;1000)
```

<!-- wq-example {"id":"broadcast-nested","cellGroup":"broadcasting","expect":{"value":"((11;12);(23;24))"}} -->
```wq
((1;2);(3;4))+(10;20)
```

Nested lists follow the same rule at each level. List operands need compatible
shapes; a mismatched length produces a `length` error.

<!-- wq-example {"id":"broadcast-length","expect":{"error":"length"}} -->
```wq
(1;2;3)+(10;20)
```

## Element-wise and Matrix Multiplication

`*` multiplies matching items. `**` treats lists as vectors or matrices instead.

<!-- wq-example {"id":"multiply-elementwise","cellGroup":"multiplication-kinds","expect":{"value":"(4;10;18)"}} -->
```wq
(1;2;3)*(4;5;6)
```

<!-- wq-example {"id":"multiply-dot-product","cellGroup":"multiplication-kinds","expect":{"value":"32"}} -->
```wq
(1;2;3)**(4;5;6)
```

The first expression multiplies item by item. The second takes the dot product:
`1*4+2*5+3*6`.

For matrices, `**` performs row-by-column multiplication:

<!-- wq-example {"id":"multiply-matrices","cellGroup":"multiplication-kinds","expect":{"value":"((19;22);(43;50))"}} -->
```wq
((1;2);(3;4))**((5;6);(7;8))
```

Matrix rows must have uniform lengths, and the two inner dimensions must match.

## Adding and Joining Lists

For lists, `+` adds matching items. The comma `,` joins lists end to end.

<!-- wq-example {"id":"lists-concatenate","cellGroup":"plus-versus-comma","expect":{"value":"(1;2;3;4)"}} -->
```wq
(1;2),(3;4)
```

<!-- wq-example {"id":"lists-add","cellGroup":"plus-versus-comma","expect":{"value":"(4;6)"}} -->
```wq
(1;2)+(3;4)
```

## Working with Bits

Bit operations are useful when an int stores flags or packed data. Binary int
literals begin with `0b`.

`band`, `bor` and `bxor` apply bitwise and, or and xor:

<!-- wq-example {"id":"bit-and-int","cellGroup":"int-bitwise-logic","expect":{"value":"2"}} -->
```wq
band[0b110;0b011]
```

<!-- wq-example {"id":"bit-or-int","cellGroup":"int-bitwise-logic","expect":{"value":"5"}} -->
```wq
bor[0b100;0b001]
```

<!-- wq-example {"id":"bit-xor-int","cellGroup":"int-bitwise-logic","expect":{"value":"6"}} -->
```wq
bxor[0b101;0b011]
```

Each accepts two or more inputs and folds from left to right. They also follow
compatible list shapes.

`not` flips every bit. For signed ints, `not[x]` equals `-x-1`:

<!-- wq-example {"id":"bit-not-int","expect":{"value":"-6"}} -->
```wq
not[5]
```

`shl` and `shr` shift bits left and right by a non-negative count:

<!-- wq-example {"id":"bit-shift-left","cellGroup":"int-bit-shifts","expect":{"value":"12"}} -->
```wq
shl[3;2]
```

<!-- wq-example {"id":"bit-shift-right","cellGroup":"int-bit-shifts","expect":{"value":"4"}} -->
```wq
shr[16;2]
```

The same `band`, `bor` and `bxor` builtins combine bools eagerly. The
[Control Flow](09-Control-Flow.md) chapter compares them with short-circuit
bool operations.

## Comparing Numbers

Comparisons produce bools, `T` or `F`. A comparison chain checks each adjacent
pair; a list comparison broadcasts.

<!-- wq-example {"id":"comparison-chain","cellGroup":"number-questions","expect":{"value":"T"}} -->
```wq
1<2<3
```

<!-- wq-example {"id":"comparison-broadcast","cellGroup":"number-questions","expect":{"value":"(F;T;T)"}} -->
```wq
(1;2;3)>1
```

`=` and `~` compare whole values for equality and inequality. Their dotted
forms, `=.` and `~.`, compare matching leaves:

<!-- wq-example {"id":"comparison-whole","cellGroup":"whole-versus-leaves","expect":{"value":"F"}} -->
```wq
(1;2;3)=(1;9;3)
```

<!-- wq-example {"id":"comparison-leaves","cellGroup":"whole-versus-leaves","expect":{"value":"(T;F;T)"}} -->
```wq
(1;2;3)=.(1;9;3)
```

## Summary

- `+ - * / % ^` are the everyday numeric operators.
- `/%` is floor division. `/.` is exact division. `^.` is exact power.
- Arithmetic broadcasts over compatible list shapes.
- `*` multiplies matching items. `**` computes vector and matrix products.
- `+` adds matching items. `,` joins lists.
- `band`, `bor`, `bxor`, `not`, `shl` and `shr` operate on bits.
- Continue to **Binding** to name and update values.
