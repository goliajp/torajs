// An object literal stored into a declared-`any` field (a class field, a local a
// generator lifts into its env) is a dynamic object like `let p: any = {}` is —
// the two spellings of one program agree: integer-key accessors, expando order,
// delete, and the prototype are the ordinary-object ones.
class K {
  p: any = null;
}
function topG(): number {
  return 8;
}
function probe(tag: string, p: any): void {
  Object.defineProperty(p, 0, { get: topG, configurable: true });
  p.b = 2;
  p.a = 1;
  delete p.b;
  console.log(tag, p[0], p["0"], JSON.stringify(Object.keys(p)), Object.getPrototypeOf(p) === Object.prototype);
}
const k = new K();
k.p = {};
probe("class-field", k.p);
k.p = { z: 9 };
console.log("class-field-2", k.p.z, k.p[0]);
function* sg(): any {
  const p: any = {};
  probe("gen-local", p);
  yield p[0];
}
for (const v of sg()) {
  console.log("yielded", v);
}
async function am(): Promise<void> {
  await 0;
  const p: any = {};
  probe("async-local", p);
  console.log("async-read", await p[0]);
}
am().then(() => {
  console.log("done");
});
