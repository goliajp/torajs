// §20.1.2.2 step 3 / §20.1.2.3.1 step 1 — a typed primitive props arg
// (As-cast pass-through keeps the literal type) must still hit the
// ToObject faces: non-empty string throws, other primitives no-op.
try {
  Object.create(null, "ab" as any);
  console.log("create no throw");
} catch (e) {
  console.log("create caught:", (e as Error).name);
}
const o = Object.create(null, "" as any);
console.log("empty-str create:", typeof o);
const o2 = Object.create(null, 5 as any);
console.log("num create:", typeof o2);
try {
  Object.defineProperties({}, "xy" as any);
  console.log("dp no throw");
} catch (e) {
  console.log("dp caught:", (e as Error).name);
}
const r = Object.defineProperties({ a: 1 }, "" as any);
console.log("empty-str dp:", (r as any).a);
