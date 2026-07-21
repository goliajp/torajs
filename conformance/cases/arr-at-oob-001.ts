// ES §23.1.3.1 step 5-6 — `at()` answers undefined outside [0, len)
// instead of reading a garbage slot (RFC 20260721-array-proto-cluster
// 刀 2 / G1; the old typed lane declared OOB as UB and SIGSEGV'd on
// an empty array).
const empty: any[] = [];
console.log(empty.at(-2)); // undefined
console.log(empty.at(0)); // undefined
console.log(empty.at(1)); // undefined

// (an f64-evidence-free `number[]` narrows to the I64 lattice whose
// OOB reads stay the loud RangeError per RFC 20260708 — same as
// `xs[i]`; only sentinel-capable widths are exercised here)
const f = [1.5, 2.5];
console.log(f.at(-1)); // 2.5
console.log(f.at(-3)); // undefined
console.log(f.at(2)); // undefined

const strs = ["a", "b"];
console.log(strs.at(1)); // b
console.log(strs.at(-2)); // a
console.log(strs.at(-3)); // undefined (Str undefined sentinel)
console.log(strs.at(2)); // undefined

const mixed: any[] = [1, "x", null];
console.log(mixed.at(-1)); // null
console.log(mixed.at(3)); // undefined
console.log(mixed.at(-4)); // undefined
