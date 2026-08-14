// 383-04 — the stage-3 upsert family: Map.getOrInsertComputed and
// the WeakMap pair, typed and any lanes, with the spec's step order
// (callable gate before the lookup on Map; weak-key gate first on
// WeakMap) and the post-callback overwrite semantics.
const m = new Map<number, string>();
m.set(1, "hit");
console.log(m.getOrInsertComputed(1, () => "nope"), m.getOrInsertComputed(2, (k) => "v" + k), m.get(2));
// non-callable callbackfn throws even on a present key
try { (m as any).getOrInsertComputed(1, 5) } catch (e) { console.log((e as any) instanceof TypeError) }
// a callback throw propagates and inserts nothing
try { m.getOrInsertComputed(3, () => { throw new RangeError("boom") }) } catch (e) { console.log((e as any).message, m.has(3)) }
// a callback that mutates the map is overwritten by the late set
console.log(m.getOrInsertComputed(4, () => { m.set(4, "inside"); return "after" }), m.get(4));
// -0 key normalizes like set
m.getOrInsertComputed(-0, () => "zero");
console.log(m.get(0));
// WeakMap pair
const wm = new WeakMap();
const k = {};
console.log(wm.getOrInsert(k, 7), wm.getOrInsert(k, 8));
console.log(wm.getOrInsertComputed(k, () => 99));
const k2 = {};
console.log(wm.getOrInsertComputed(k2, () => 42), wm.get(k2));
try { (wm as any).getOrInsert(1, "x") } catch (e) { console.log((e as any) instanceof TypeError) }
try { (wm as any).getOrInsertComputed(k2, 5) } catch (e) { console.log((e as any) instanceof TypeError) }
// any lane rides the same cores, and the names enumerate
const am: any = new Map();
console.log(am.getOrInsertComputed("z", (key: any) => key + "!"), am.get("z"));
const awm: any = new WeakMap();
console.log(awm.getOrInsert(k, "w"), awm.getOrInsertComputed(k2, () => "c"));
console.log(Object.getOwnPropertyNames(Map.prototype).includes("getOrInsertComputed"));
console.log(Object.getOwnPropertyNames(WeakMap.prototype).sort().join(","));
