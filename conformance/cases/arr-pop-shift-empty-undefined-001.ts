// `pop` and `shift` on an empty array answer `undefined` (ES §23.1.3.22
// step 3.a, §23.1.3.25 step 3.a). A `string[]` and an array of heap
// elements already did; an all-integral `number[]` answered the slot's
// zero, because i64 has no bit pattern to spare for that answer — the
// same reason an out-of-range read used to raise instead of answering.
//
// The tell was that the very same array holding one fraction already
// answered `undefined`: the wide slot has the sentinel, so only the
// narrow one was ever wrong.

const es: number[] = [];
console.log(es.pop());
console.log(es.shift());
console.log(es.length);

// draining to empty, then one past
const ys: number[] = [1, 2];
console.log(ys.pop(), ys.pop(), ys.pop());
console.log(ys.length);

const zs: number[] = [3, 4];
console.log(zs.shift(), zs.shift(), zs.shift());

// a fractional element has always answered this
const fs: number[] = [1.5];
console.log(fs.pop(), fs.pop());

// the non-numeric element classes were never affected
const ss: string[] = [];
console.log(ss.pop(), ss.shift());
const ns: number[][] = [];
console.log(ns.pop(), ns.shift());

// the answer still compares as undefined does, and arithmetic on it is
// a plain NaN
const gs: number[] = [];
console.log(gs.pop() === undefined);
console.log(gs.shift() + 1);

// a real value still reads back as itself
const hs: number[] = [7, 8, 9];
console.log(hs.pop(), hs.shift(), hs.length);
