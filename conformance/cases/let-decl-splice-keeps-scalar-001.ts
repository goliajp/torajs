// A `let` whose initializer arrives on the next line is spliced back
// onto the declaration (`let x: number; x = 7;` -> `let x: number = 7`).
// After that splice the binding has an initializer, so the "no
// initializer holds undefined" annotation must NOT be applied to it:
// no reachable code can observe it holding undefined, and wrapping it
// anyway costs the whole scalar lane (a `number | undefined` slot is a
// NaN-boxed any).

// The shape that pays for it: an accumulator declared on its own line.
let sum: number;
sum = 0;
for (let i = 0; i < 5; i++) {
  sum += i;
}
console.log(sum);

let prod: number;
prod = 1;
for (let i = 1; i < 5; i++) {
  prod = prod * i;
}
console.log(prod);

// Straight-line arithmetic on the same shape.
let x: number;
x = 3;
console.log(x + 1, x * 2, x - 1);

// A string binding splices the same way.
let s: string;
s = "ab";
console.log(s + "c", s.length);

// The splice only happens when nothing reads the name in between.
// A read before the write keeps the declaration uninitialized, and
// there the annotation still applies: the read answers undefined.
let seen: number;
console.log(seen);
seen = 9;
console.log(seen);

// No follow-up write at all: still undefined, never the type's zero.
let never: number;
console.log(never);

let neverStr: string;
console.log(neverStr);

class C {
  m(): number {
    return 1;
  }
}
let c: C;
console.log(c);

// `let x!: T` asserts the answer to TS's definite-assignment question
// and has no runtime face — same undefined.
let asserted!: number;
console.log(asserted);

// A hand-written `T | undefined` is already nullable; the splice does
// not narrow the declared type away.
let hand: number | undefined;
console.log(hand);

// §14.3.2 hoists a `var` binding, so it reads undefined before its
// write whether or not the splice fired.
var v: number;
console.log(v);
var w: number;
w = 4;
console.log(w + 1);

// Splice moves the declaration down to the write rather than lifting
// the value up, so intervening side effects stay in program order.
const order: number[] = [];
let moved: number;
order.push(1);
moved = order.push(2);
console.log(moved, order.join(","));
