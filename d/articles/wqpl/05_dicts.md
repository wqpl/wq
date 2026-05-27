# Dicts

A **dict** stores key-value pairs, preserving insertion order.

## Symbols

Dict keys are (and can only be) **symbols**.

A symbol starts with a backtick:

```wq
a_symbol: `abc
echo a_symbol
echo type a_symbol
echo #a_symbol
```

## Constructing a dict

Use parentheses with `` `key:value `` pairs:

```wq
person:(`name:"Alex";`age:25;`active:true)
echo person
```

Values can be anything, including nested dicts and lists:

```wq
item:(`id:1001;
  `tags:("new";"sale");
  `meta:(`stock:20;`brand:"D"))
```

## Indexing by symbol

The most common way to read a dict is by symbol key:

```wq
person:(`name:"Alex";`age:25)
echo person[`name]
echo person[`age]
```

## Indexing by position

Because dicts preserve insertion order, you can also look up values by their integer index:

```wq
person:(`name:"Alex";`age:25;`active:true)
echo person[0]
echo person[1]
echo person[-2..=-1]
```

## Mutating entries

You can update an entry by indexing into the dict:

```wq
person:(`name:"Alex";`age:25)
person[`age]:26
echo person
```

## Dicts in practice

Dicts are useful for representing row-like data:

```wq
employee:(`id:1;`name:"Steve";`dept:`HR)
```

Lists of dicts for table-like collections:

```wq
employees:(
  (`id:1;`name:"Steve";`age:30);
  (`id:2;`name:"Alex";`age:25))
showtable employees
```

## Summary

- A backtick creates a symbol: `` `name ``
- ``(`key:value;...)`` creates a dict.
- `dc[k]` retrieves the value:
  - associated with the key `k`, if `k` is a symbol.
  - at index `k`, if `k` is an integer.
