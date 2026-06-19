// ES §10.4.2.5 step 4 — `arr.length = N` truncates the array when
// N < oldLen. typed `Array<I64|F64|Bool>` is the narrow scope here:
// scalar slots need no per-element drop, just a `len` write.
// Refcounted element types (Str / Substr / Arr / Obj / ...) still
// silent no-op on truncate — that's a substrate-mid follow-up.

// I64 truncate.
const a = [1, 2, 3, 4, 5];
a.length = 3;
console.log(a);
console.log(a.length);

// F64 truncate.
const b = [1.5, 2.5, 3.5, 4.5];
b.length = 2;
console.log(b);
console.log(b.length);

// Bool truncate.
const c = [true, false, true, false];
c.length = 1;
console.log(c);
console.log(c.length);

// length = 0 empties the array.
const d = [1, 2, 3];
d.length = 0;
console.log(d);
console.log(d.length);

// length = oldLen is a no-op.
const e = [10, 20, 30];
e.length = 3;
console.log(e);
console.log(e.length);

// Invalid length throws RangeError (unchanged from validate-only path).
try {
  const g = [1, 2, 3];
  g.length = -1;
  console.log("no throw, g.length =", g.length);
} catch (err) {
  console.log("threw:", (err as Error).message);
}

try {
  const h = [1, 2, 3];
  h.length = 1.5;
  console.log("no throw, h.length =", h.length);
} catch (err) {
  console.log("threw:", (err as Error).message);
}

// Truncated array is still mutable (push after truncate appends from
// new len, not old).
const j = [1, 2, 3, 4, 5];
j.length = 2;
j.push(99);
console.log(j);
console.log(j.length);
