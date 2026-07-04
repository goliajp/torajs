// Any-method-call RFC 20260704 C4 — Map/Set methods on any
// receivers: get / set / has / delete / add / clear over number and
// string keys, heap values, chained set, and cross-kind misses.
const m: any = new Map();
m.set("k", 42);
console.log(m.get("k"));
console.log(m.get("missing"));
console.log(m.has("k"));
console.log(m.has("zz"));
m.set("k", 43);
console.log(m.get("k"));
m.set(7, "seven");
console.log(m.get(7));
m.set("arr", [1, 2]);
console.log(m.get("arr").length);
console.log(m.set("a", 1).set("b", 2).get("b"));
console.log(m.delete("k"));
console.log(m.delete("k"));
console.log(m.get("k"));
m.clear();
console.log(m.has("a"));
const s: any = new Set([1, 2]);
console.log(s.has(1));
console.log(s.has(9));
s.add(3);
console.log(s.has(3));
s.add(3);
console.log(s.delete(3));
console.log(s.has(3));
s.add("str");
console.log(s.has("str"));
s.clear();
console.log(s.has(1));
try {
  m.add(1);
} catch (e) {
  console.log("map add threw");
}
try {
  s.get(1);
} catch (e) {
  console.log("set get threw");
}
