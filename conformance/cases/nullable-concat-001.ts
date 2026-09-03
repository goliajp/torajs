// The `+` half of the same question `String(q)` answers: with a
// String on one side, §13.15.3 always concatenates, and ToString
// answers for BOTH faces of a `T | null` — null → "null", the value →
// its own ToString. So the result is String either way, and which T
// it is has nothing to do with it.
//
// The operand check had been pinned to `Nullable(Array)` since the
// arm was written for an un-narrowed `match` result, so `"" + q`
// was refused on a struct, a string and a closure alike. And the
// lowering guard that goes with it sniffed the SOURCE shape rather
// than reading the type, so an ordinary `const a: number[] | null =
// null` slipped past both: it compiled, and printed nothing at all.

type O = { x: number };
const q: O | null = null;
console.log("" + q);
console.log(q + "");
console.log("v:" + q);

const a: number[] | null = null;
console.log("" + a);
console.log("a=" + a);

const s: string | null = null;
console.log("" + s);
console.log("a" + s);
console.log(s + "a");

type F = (n: number) => number;
const f: F | null = null;
console.log("" + f);

// the two scalars ride the any lane and always answered this
const n: number | null = null;
console.log("" + n);
const b: boolean | null = null;
console.log("" + b);

// a slot written both ways
let m: number[] | null = null;
console.log("m=" + m);
m = [7, 8];
console.log("m=" + m);

let t: string | null = null;
console.log("t=" + t);
t = "zz";
console.log("t=" + t);

// template substitution takes the same path
let u: string | null = null;
console.log(`u=${u}`);
const v: number[] | null = null;
console.log(`v=${v}`);

// the shape the arm was originally written for still works
const hit = "abc".match(/b/);
console.log("got: " + hit);
const miss = "abc".match(/z/);
console.log("got: " + miss);

// ordinary concatenation is untouched
const p = "x";
const r = "y";
console.log(p + r, p + 1, 1 + p);
const long = "hello";
console.log(long.slice(1) + long.slice(0, 2));
console.log("n=" + 3, "b=" + true, "u=" + undefined, "z=" + null);
