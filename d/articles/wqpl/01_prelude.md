# The wq Prelude

wq is a small programming language built around expressions, lists, and functions.

## bfn `echo`

"bfn" = "built-in function", a tool wq provides out of the box.

```wq
echo "hello!"
echo (1+1)
echo echo
```

Put anything after `echo`, and wq shows it back to you.

If you want to `echo` the result of an arithmetic expression, you usually have to wrap the expression in parentheses.
Otherwise, wq can get confused.

```wq
echo(1*2-3)
// You will not get what you want without the parentheses
echo 1*2-3
```

To avoid this, wq provides a convenient alternative syntax, `|echo`.
You can append it to the end of any expression without needing parentheses.

```wq
1*2-3 |echo
```

## Comments

A double slash `//` starts a comment that continues until the end of the line, except inside a string.

wq ignores everything in comments.

```wq
// a comment
echo "https://example.org" // a trailing comment
```

## Lists

A **list** is an ordered collection of any values.

Construct a list with parentheses `()` and separate elements with semicolons `;`:

```wq
echo (1;2;3.1;"d")
echo (1;(2;3)) // a nested list
echo ((1;2);(3;4))
```

## Broadcasting

So, what happens if you add two lists?

Python "concatenates" them (glues them together):

```python
[1,2,3] + [4,5,6]  # => [1,2,3,4,5,6]
```

But wq adds the numbers _elementwise_ (a.k.a. broadcasting):

```wq
(1;2;3)+(4;5;6)
```

What if list lengths do not match? Try the run button:

```wq error
(1;2;3)+(4;5;6;7)
```

To concatenate lists in wq, use the comma `,` operator:

```wq
(1;2;3),(4;5;6)
```

## Chars and strings

A _char_ is a character defined by one Unicode codepoint (an English letter, a Chinese character, a kana, ...).

A _string_ is a list of chars.

In wq, both chars and strings are always written using double quotes ("). wq looks at the length of the text: if it is one codepoint, it is a char; if it is multiple, it is a string.

Note that some "characters" like emojis with skin tone variations consist of multiple Unicode codepoints and are therefore strings.

```wq
// chars
echo "a"
echo "我"
echo "🤯"
// strings
echo "me"
echo "私たち"
```

## Summary

- `echo x` prints `x`. Use `expr |echo` when `expr` is an arithmetic expression.
- `+` performs math and does not glue lists together.
- `,` glues lists.

## Practice

Guess the results before clicking the run button:

```wq
// result?
(2;4;6)*2             |echo
(1;2;3)+(10;10;10)    |echo
((1;2);(3;4))+(10;20) |echo
```
