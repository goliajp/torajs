// 405-06 — a Map / Set / WeakMap / WeakSet prototype method rebound
// onto a receiver of another brand is a TypeError (§24.x.3 internal
// slots); the mid re-dispatch used to route by receiver family and
// silently run the receiver's semantics.

// WeakMap method onto a Map — the pair of test262 cases that
// exposed this ran the Map upsert silently
const m: any = new Map();
const wgi: any = (WeakMap.prototype as any).getOrInsert;
try {
  wgi.call(m, {}, 1);
  console.log("no throw", m.size);
} catch (e) {
  console.log("threw", m.size);
}

// Map method onto a Set
const s: any = new Set([1]);
const mhas: any = (Map.prototype as any).has;
try {
  console.log(mhas.call(s, 1));
} catch (e) {
  console.log("threw2");
}

// Set method onto a Map
const sadd: any = (Set.prototype as any).add;
try {
  sadd.call(m, 9);
  console.log("no throw3", m.size);
} catch (e) {
  console.log("threw3", m.size);
}

// non-object receivers refuse too
const mget: any = (Map.prototype as any).get;
try {
  mget.call(7, 1);
  console.log("no throw4");
} catch (e) {
  console.log("threw4");
}

// same-brand rebinding still works
const mm: any = new Map([[1, 2]]);
console.log(mget.call(mm, 1));
const wm: any = new WeakMap();
const k: any = {};
console.log(wgi.call(wm, k, 42));

// inherited Object.prototype surface borrowed through a collection
// prototype is NOT brand-locked
const ts: any = (Map.prototype as any).toString;
console.log(typeof ts.call({}));
