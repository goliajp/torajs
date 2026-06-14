// W-D — Object.getOwnPropertyDescriptor(undefined/null) TypeError msg prefix
// aligned to bun JSC. bun full msg shape: `<type> is not an object (evaluating
// '...')`. We assert prefix only — the `(evaluating ...)` suffix requires
// source-text threading through the throw helper (substrate work, deferred).
//
// Inlined top-level try-catch (no `function check(v: any, ...)` wrapper) —
// passing `undefined`/`null` through a `v: any` function param triggers a
// separate SSA-lower wedge where both branches resolve to the null-arm throw
// (likely arg-coercion or call-shape inference root, distinct from the class
// field Map/Set member-call shape wedge already in L3b).

try {
  Object.getOwnPropertyDescriptor(undefined, "x");
  console.log("no-throw-undef");
} catch (e: any) {
  const m: string = e.message as string;
  console.log(m.startsWith("undefined is not an object") ? "OK" : "FAIL: " + m);
}

try {
  Object.getOwnPropertyDescriptor(null, "x");
  console.log("no-throw-null");
} catch (e: any) {
  const m: string = e.message as string;
  console.log(m.startsWith("null is not an object") ? "OK" : "FAIL: " + m);
}
