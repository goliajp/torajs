// Runtime backstop for typed containers the any-escape analysis does
// not see reaching the any world. An array that comes back out of
// `Object.values`, and a typed Map<K, number> written an any value,
// keep their typed layout; a write of another kind through the any
// side must fail loudly with a TypeError instead of storing bits the
// typed readers would misread. bun stores both writes; the .expected
// is written by hand and becomes bun's output once these paths are
// tracked.
const c: number[] = [1, 2];
const o = { k: c };
const viaValues = Object.values(o)[0] as any;
try {
  viaValues.push("v");
  console.log("pushed");
} catch (e) {
  console.log("push refused", e instanceof TypeError);
}
try {
  viaValues[0] = "w";
  console.log("wrote");
} catch (e) {
  console.log("write refused", e instanceof TypeError);
}
console.log(c.length, c[0], c[1]);
viaValues.push(3);
console.log(c);

const m = new Map<string, number>();
function put(v: any): void {
  m.set("k", v);
}
try {
  put("str");
  console.log("stored", m.get("k"));
} catch (e) {
  console.log("set refused", e instanceof TypeError);
}
put(4);
console.log(m.get("k"), m.size);
