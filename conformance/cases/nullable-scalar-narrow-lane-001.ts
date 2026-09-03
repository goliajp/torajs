// A narrow says what a binding can hold from here on. It cannot say
// where the binding lives: the slot was picked once, from the
// annotation, and it has to outlast the guard — `n` must still be
// able to hold `null` after the branch ends, so a scalar `T | null`
// keeps the NaN-boxed `any` slot the box tax bought it.
//
// So narrowing `number | null` to `number` was a false statement
// about representation, and the sites that pick a lane read exactly
// that type to pick it: the array-literal gate asks whether an
// element materializes boxed, saw a bare `number`, took the typed
// lane, and stored box bits into 8-byte slots that every reader
// decodes as 16-byte tagged pairs. Every element read back
// `undefined` — no error anywhere, just wrong answers.
//
// The canonical null guard, the early-return guard, the optional
// parameter guard and straight-line assignment all reached it.

// The canonical guard.
const a: number | null = 3;
if (a !== null) {
  console.log("guard:", [a, 1][0], [a, 1][1]);
}

// The early-return guard: the rest of the body is narrowed.
function early(n: number | null): void {
  if (n === null) {
    console.log("early: none");
    return;
  }
  console.log("early:", [n, 1][0]);
}
early(3);
early(null);

// An optional parameter is the same annotation by another spelling.
function opt(n?: number): void {
  if (n !== undefined) {
    console.log("opt:", [n, 9][0]);
  }
}
opt(4);

// Boolean is the other scalar with no spare bit pattern.
const b: boolean | null = true;
if (b !== null) {
  console.log("bool:", [b, false][0], [b, false][1]);
}

// Straight-line assignment narrows too — and the read before it is
// what keeps the uninit-let splice from folding the write into the
// declaration.
let c: number | null;
console.log("pre:", c);
c = 3;
console.log("assign:", [c, 1][0], [c, 1][1]);

// A later `= null` restores the union, and the slot was always able
// to hold it.
c = null;
console.log("renull:", c, [c, 1][0]);

// What the narrow is FOR still works: the value is usable as the
// scalar it holds.
let d: number | null;
console.log("pre:", d);
d = 3;
const y: number = d;
console.log("uses:", y, d * 2, d + 1, d > 1);
const o = { v: d };
console.log("field:", o.v);
let m = d;
console.log("copy:", m + 1);

// Nested and mixed literals take the same lane.
const e: number | null = 5;
if (e !== null) {
  console.log("nested:", [[e, 1], [2, 3]][0][0], [e, "s", true][0]);
}

// A pointer-shaped `T | null` never had this problem — its slot and
// its narrowed slot are the same slot — and still does not.
const s: string | null = "hi";
if (s !== null) {
  console.log("str:", [s, "x"][0], s.length);
}
