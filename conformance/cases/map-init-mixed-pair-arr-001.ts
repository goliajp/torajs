// `new Map(pairs)` where the pair arrays hold mixed element types —
// those inner arrays are Array<Any>, whose slots are one 8-byte
// NaN-boxed AnyValue each. Scaling them by 16 reads the value one
// slot past the pair, which is invisible at index 0 (the key still
// lands) and silently nulls every value.
const sn: [string, number][] = [["y", 2], ["z", 3]];
const m = new Map(sn);
console.log("size", m.size);
console.log("get", m.get("y"), m.get("z"));
for (const k of m.keys()) {
  console.log("k", k, typeof k);
}
for (const v of m.values()) {
  console.log("v", v, typeof v);
}

// number key, string value — the mirror shape
const ns: [number, string][] = [[1, "y"]];
console.log("ns", new Map(ns).get(1));

// homogeneous pairs stay on the typed slot stride
const ss: [string, string][] = [["y", "2"]];
console.log("ss", new Map(ss).get("y"));

// a var-bound (erased) source reaches the same entries
var loose = [["a", 1]];
const m2 = new Map(loose);
console.log("loose", m2.get("a"), m2.size);
