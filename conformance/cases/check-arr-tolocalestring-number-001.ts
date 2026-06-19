// ES §22.1.3.32 step 5.b — `Array<number>.toLocaleString()` formats
// each element via `Number.prototype.toLocaleString`. With macOS
// host DefaultLocale = en-US, the integer part is group-separated
// with `,` (`1000` → `"1,000"`); the joined output therefore differs
// from `.toString()` for numeric arrays.

// Integer array: each elem groups, comma-separated.
const a = [1000, 2000, 3000];
console.log(a.toLocaleString());

// Float array (mixed group thresholds).
const b = [1234567.89, 0.5, -1500];
console.log(b.toLocaleString());

// Single elem.
const c = [9876543];
console.log(c.toLocaleString());

// Empty array.
const d: number[] = [];
console.log("[" + d.toLocaleString() + "]");

// String array still uses toString-equivalent toLocaleString.
const e = ["abc", "def"];
console.log(e.toLocaleString());

// Bool array unchanged.
const f = [true, false];
console.log(f.toLocaleString());

// `.toString()` of numeric array still uses raw Number.toString
// (no group separator) — verifies we routed only the locale path.
console.log(a.toString());
console.log(b.toString());

// `.join("|")` keeps the join separator + raw ToString per spec.
console.log(a.join("|"));

// Sub-1000 ints don't change.
const g = [1, 50, 999];
console.log(g.toLocaleString());
