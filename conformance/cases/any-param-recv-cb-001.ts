// A lifted arrow's untyped param is an `any` receiver (r293): the
// member-call fn-name argument must ride the wrapped closure lane —
// pre-fix the raw FnSig hit box_to_any inside the any-method argv
// (t262 staging/sm lazy-methods-iterator-closed-on-call-throws).
function fn(x: any): any {
  throw new Error("boom");
}
const methods = [(iter) => iter.map(fn), (iter) => iter.filter(fn)];
for (const method of methods) {
  const src: any = [1, 2, 3].values();
  try {
    method(src).next();
    console.log("no-throw");
  } catch (e: any) {
    console.log("caught", e.message);
  }
}

// explicit `any` ann rides the same frame
const g = (r: any) => r.map(fn);
const src2: any = [9].values();
try {
  g(src2).next();
} catch (e: any) {
  console.log("caught2", e.message);
}
