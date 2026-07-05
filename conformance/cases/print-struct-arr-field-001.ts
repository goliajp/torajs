// L3b #6 crash fix — typed scalar arrays stored into struct fields
// carry their elem kind (same mark as the any-boxing boundary), so
// the inspect field walker dispatches to the typed inline printers
// instead of NaN-box-walking raw i64/f64/bool slots (pre-fix:
// SIGSEGV on the first small-int deref).

// anon struct, i64-array field (the minimal crash repro).
console.log({ a: [1, 2] });

// anon struct, arr field + trailing str field.
console.log({ a: [1, 2], b: "x" });

// f64 / bool element kinds through the same walker.
console.log({ f: [1.5, 2.5], g: [true, false] });

// string-array field (heap cells — worked pre-fix, regression guard).
console.log({ s: ["x", "y"] });

// class instance with arr fields assigned in the constructor
// (member-assign store path, not ObjectLit).
class C {
  xs: number[];
  label: string;
  constructor() {
    this.xs = [10, 20, 30];
    this.label = "c-label";
  }
}
console.log(new C());

// arr field read back out and printed directly.
const o = { flags: [true, false], nested: [[1], [2]] };
console.log(o.flags);
console.log(o.nested);

// field reassignment (assign-member path on an existing instance).
const c2 = new C();
c2.xs = [7, 8];
console.log(c2.xs);
console.log(c2);
