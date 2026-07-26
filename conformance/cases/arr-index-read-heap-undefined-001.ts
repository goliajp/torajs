// A `find` miss on an array of pointer-shaped elements has answered
// `undefined` with the generic immortal cell for a while. Reading past
// the end of that very same array threw a RangeError instead — the two
// are the same question (ES §10.4.2.1), so they now take the same exit.

type P = { a: number };

const objs: P[] = [{ a: 1 }];
console.log(objs.find((x) => x.a > 99));
console.log(objs[5]);
console.log(objs.at(9));

const nested: number[][] = [[1], [2]];
console.log(nested.find((x) => x[0] > 99));
console.log(nested[7]);
console.log(nested.at(-9));

const fns: ((n: number) => number)[] = [(n) => n + 1];
console.log(fns[3]);

// In-range reads are untouched.
console.log(objs[0].a, nested[1][0]);

// A negative index is out of range the same way a too-large one is.
console.log(nested[-1]);

// The element families that already answered undefined still do.
const nums: number[] = [1];
const strs: string[] = ["p"];
console.log(nums[9], strs[9]);

// Handing one out of a function keeps it, and so does a binding.
function pick(xs: P[], i: number): P {
  return xs[i];
}
console.log(pick(objs, 9));
const held = nested[9];
console.log(held);
