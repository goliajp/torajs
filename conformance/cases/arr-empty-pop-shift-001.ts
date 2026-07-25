// `pop` / `shift` on an empty array answer `undefined` (§23.1.3.20
// step 4.a / §23.1.3.25 step 3.a), not the element type's zero.
//
// tr stored `null` in a pointer-shaped result slot there, so
// `s.pop() === undefined` answered false and `typeof s.pop()` answered
// "string". Each width that has a way to spell undefined now uses it —
// the per-type immortal cell, the same one an optional field or a
// `find` miss hands out — and the three "may this hold the sentinel"
// predicates recognise `pop`/`shift` alongside `find`/`findLast`.
//
// A `number[]` still answers 0 here: its element slot narrows to I64,
// which has no bit pattern to spare. That is the same open gap as an
// out-of-range `number[]` read, not a decision about this exit.

const strs: string[] = [];
console.log(strs.pop());
console.log(strs.shift());
console.log(typeof strs.pop());
console.log(strs.pop() === undefined);
console.log(strs.length);

type Point = { v: number };
const pts: Point[] = [];
console.log(pts.pop());
console.log(pts.shift());
console.log(pts.pop() === undefined);

const nested: string[][] = [];
console.log(nested.pop());
console.log(nested.shift());

// draining down to empty crosses the boundary in one program
const drain: string[] = ["a", "b"];
console.log(drain.pop(), drain.pop(), drain.pop());
console.log(drain.length);

const q: string[] = ["x"];
console.log(q.shift(), q.shift());
console.log(q.length);

// a real element that could be confused with the sentinel stays itself
const empties: string[] = [""];
console.log(empties.pop() === undefined);
