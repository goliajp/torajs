// T-48 — Array<Any>.includes/indexOf with primitive needle. Pre-fix
// tora emitted `ICmp(Ptr, I64)` for the per-element compare loop,
// which LLVM verify rejected with "Both operands to ICmp instruction
// are not of the same type!". Fix routes Array<Any> compares through
// the `any_strict_eq` / `any_any_strict_eq` runtime helpers — same
// path BinOp Any === concrete already uses. Unblocks 3 test262
// cases under built-ins/Array/prototype/{includes,indexOf,lastIndexOf}.

const xs: any[] = [1, "hi", true, 3.14, null];

// includes — primitive needle against Any-element.
console.log(xs.includes(1));      // true
console.log(xs.includes("hi"));   // true
console.log(xs.includes(true));   // true
console.log(xs.includes(3.14));   // true
console.log(xs.includes(null));   // true
console.log(xs.includes(2));      // false
console.log(xs.includes("nope")); // false

// indexOf — same compare path.
console.log(xs.indexOf(1));       // 0
console.log(xs.indexOf("hi"));    // 1
console.log(xs.indexOf(3.14));    // 3
console.log(xs.indexOf(99));      // -1

// lastIndexOf — same compare path, reverse scan.
console.log(xs.lastIndexOf(true)); // 2
console.log(xs.lastIndexOf("hi")); // 1
console.log(xs.lastIndexOf("xx")); // -1

// Any === Any compare (needle is also Any-boxed) — array must be
// actually-heterogeneous so the literal lowering box-tags each elem.
// Homogeneous-primitive `[42, 42, 42]: any[]` hits a separate
// pre-existing literal-lowering bug (elements stay raw I64 instead
// of being Any-boxed) and is left alone here.
const mix: any[] = [42, "hi", true, 42];
const needle: any = 42;
console.log(mix.indexOf(needle));      // 0
console.log(mix.lastIndexOf(needle));  // 3
