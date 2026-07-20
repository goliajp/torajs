// RFC 20260721-builtin-method-reflection 刀 6 — Map.prototype
// .getOrInsert (stage-3 upsert, bun ships it): present key answers
// its current value untouched; missing key inserts the default and
// answers it. Typed + any lanes + name/length reflection.
const m = new Map();
m.set("a", 1);
console.log(m.getOrInsert("a", 99));
console.log(m.getOrInsert("b", 42));
console.log(m.get("b"));
console.log(m.size);
const mAny: any = new Map();
console.log(mAny.getOrInsert("x", "v"));
console.log(mAny.getOrInsert("x", "other"));
console.log(mAny.size);
const f: any = mAny.getOrInsert;
console.log(typeof f, f.name, f.length);
