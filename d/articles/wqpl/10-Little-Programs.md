# Little Programs

The pieces are small, but they compose quickly.

## A Tiny Report

```wq
scores:(10;20;30;40)
report:(`scores:scores;`total:sum scores;`best:max scores)
report|echo
```

The list holds the data. The dict names the result. Nothing needs a ceremony.

## A Converter

```wq
temps:(18;21;19;24)
temps|map{x*9/5+32}|echo
```

Arithmetic broadcasts inside the mapper, and the pipe keeps the path visible.

## A Small Recursion

```wq
fib:{[n]$[n<=1;n;fib[n-1]+fib[n-2]]}
0..=8|map fib|echo
```

This is not the fastest Fibonacci in the world. It is here because it is readable.

## A Good Next Habit

When a wq idea feels tangled, turn it into a value you can print:

```wq
xs:1..=5
squares:xs|map{x*x}
(`xs:xs;`squares:squares;`total:sum squares)|echo
```

Most debugging starts there: name the middle, look at the shape, keep going.

## Keep

- Build values first.
- Use lists and dicts to make shapes visible.
- Use functions when a transformation deserves a name.
- Use pipes when a story wants to flow left to right.
