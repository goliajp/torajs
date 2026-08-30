# array-copy-1m

Fill an array with 10M `push` calls, then copy it into a second array
whose loop bound is the first array's `length`.

## Workload

```ts
let src: number[] = [];
for (let i: number = 0; i < 10000000; i++) {
  src.push(i);
}
let dst: number[] = [];
for (let i: number = 0; i < src.length; i++) {
  dst.push(src[i]);
}
console.log(dst.length + dst[9999999]);  // 19999999
```

## Why this cell

Two blind spots, one shape.

The spelling is the first. Every other cell in the suite steps its
counter with `i = i + 1`. The push-loop pre-reserve fast path stopped
recognising `i++` the day postfix increment became its own AST node,
and a 10M-append loop paid 7.7x for it — invisibly, because no
benchmark here was written the way people actually write the loop.

The bound is the second. `src.length` does not change while `dst` is
being filled, so the reservation above the second loop would be sound;
but proving it needs to rule out `src` and `dst` naming the same array,
which the lanes cannot do yet. So the copy loop runs the cap-checked
push path — one runtime call per element instead of a slot store,
measured at 7x on this shape. This cell is where that debt shows up
if it is ever paid, and where a regression shows up if the reservation
is ever handed out without the proof.
