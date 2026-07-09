// chunk 739 — array literal with Any-typed elements mints FLAG_ARR_ANY
// (pre-fix the all-Any literal fell to the typed 8-byte lane while
// every Arr<Any> reader decodes 16-byte tagged slots — [x][0] answered
// undefined for any x)
const x: any = 42;
const a1 = [x];
console.log(a1[0]);

const s: any = "str";
const a2 = [s];
console.log(a2[0]);

const d: any = { a: 1 };
const a3 = [d];
const e: any = a3[0];
console.log(e.a);

const nested: any = [1, 2];
const a4 = [nested];
const f: any = a4[0];
console.log(f[1]);

// any anchor mixed with a typed tail element
const mixed = [x, 7];
console.log(mixed[0], mixed[1]);

// length + for-of over the any-elem literal
const many = [x, s, d];
console.log(many.length);
for (const it of many) {
  console.log(typeof it);
}

// infer-widened undefined stays on the typed Str sentinel lane
const w = ["a", undefined];
console.log(w[0], w[1]);
