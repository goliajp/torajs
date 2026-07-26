// S2.24 (RFC 20260727-dstr-assignment 刀 1) — defaults, holes and
// rest in array destructuring assignment. Default semantics per ES
// §13.15.5.3: fires past-end and on an explicit undefined element;
// an explicit null keeps null.

// the 654-case element shape, statement form: past-end fires w's
// default, present value wins over v's
let v = 0;
let w = 0;
[v = 10, w = 11] = [2];
console.log(v, w); // 2 11

// hole in the pattern — position advances, nothing binds
let x = 0;
[, x] = [1, 2];
console.log(x); // 2

// the full test262 dstr-assignment element face in one statement:
// present value / explicit null keeps null / source hole fires the
// default / explicit undefined fires it / past-end fires it
// (implicit-any targets, heterogeneous Any source — the real lane).
// NOTE: a MONOMORPHIC `= [undefined]` / `= [null]` source still hits
// the pre-existing ternary-unification reject ("branches differ —
// Undefined vs Number"), same as the declaration form
// (`let [u = 13] = [undefined]`) — recorded hole, not this blade.
let e2;
let eNull;
let eHole;
let eUndef;
let eOob;
[e2 = 10, eNull = 11, eHole = 12, eUndef = 13, eOob = 14] = [2, null, , undefined];
console.log(e2, eNull, eHole, eUndef, eOob); // 2 null 12 13 14

// trailing rest takes the slice tail
let first = 0;
let r;
[first, ...r] = [1, 2, 3];
console.log(first, r[0], r[1], r.length); // 1 2 3 2
