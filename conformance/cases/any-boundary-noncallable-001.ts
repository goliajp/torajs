// 403-03 — a NON-callable any value handed back through a fn-typed
// return boundary: calling the result must be a catchable TypeError
// (the sentinel + undefable-heap-guard pair), never a crash; a real
// closure passes through the same kernel untouched.
function take(f: any): (a: number) => number { return f }
try {
  const g = take(5 as any);
  console.log(g(1));
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
const ok = take(((x: number) => x + 1) as any);
console.log(ok(41));
try {
  take({ a: 1 } as any)(2);
} catch (e) {
  console.log("obj-caught", e instanceof TypeError);
}
try {
  take("s" as any)(3);
} catch (e) {
  console.log("str-caught", e instanceof TypeError);
}
