// §7.3.15 SetIntegrityLevel over an array's ELEMENT domain, and
// §10.4.2.1's non-writable refusal. `Object.freeze(a)` marked the
// header and stopped there: the per-entry walk it pairs with visits
// dynobj buckets, and elements are not buckets. So a frozen array
// reported `writable: true / configurable: true`, `delete a[i]`
// succeeded, and `a[i] = v` MUTATED it — all while `Object.isFrozen`
// answered true. A `writable: false` index was equally writable.
const a: any = [1, 2, 3];
Object.freeze(a);
console.log(Object.isFrozen(a));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(a, "1")));
try { a[1] = 99; console.log("wrote", a[1]); } catch (e) { console.log("write threw", (e as any).constructor.name, a[1]); }
try { delete a["1"]; console.log("deleted", a[1]); } catch (e) { console.log("delete threw", (e as any).constructor.name, a[1]); }

// seal clears configurable only — the elements stay writable
const s: any = [1, 2, 3];
Object.seal(s);
console.log(Object.isSealed(s));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(s, "1")));
s[1] = 9;
console.log(s[1]);
try { delete s["1"]; console.log("sealed deleted"); } catch (e) { console.log("sealed delete threw", (e as any).constructor.name); }

// an explicit non-writable index refuses the store on its own
const d: any = [1, 2, 3];
Object.defineProperty(d, "1", { writable: false });
try { d[1] = 99; console.log("wrote", d[1]); } catch (e) { console.log("write threw", (e as any).constructor.name, d[1]); }
// its siblings are untouched
d[0] = 7;
console.log(d[0], d[2]);

// a plain array is unaffected in every direction
const p: any = [1, 2, 3];
p[1] = 99;
p[0] = 7;
console.log(p[0], p[1], p.length);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(p, "0")));
console.log(delete p["1"], p[1], p.length);

// an out-of-range index owns nothing, so a frozen array deletes it
// to true rather than refusing
const f: any = [1, 2];
Object.freeze(f);
console.log(delete f["9"]);

// a HOLE is absent, not read-only: the store re-creates it while the
// array is still extensible (the first cut of the write guard read a
// hole's flags word as "not writable" and refused every revive)
const h: any = [1, 2, 3];
delete h["1"];
h[1] = 5;
console.log(h[1], JSON.stringify(Object.getOwnPropertyDescriptor(h, "1")));

// ...but creating one back is a store into a non-extensible cell once
// the array is frozen, so that refusal stands
const hf: any = [1, 2, 3];
delete hf["1"];
Object.freeze(hf);
try { hf[1] = 5; console.log("hole wrote", hf[1]); } catch (e) { console.log("hole write threw", (e as any).constructor.name, hf[1]); }

// the typed tier stores straight into the slot instead of going
// through the any-lane kernel, so it needs the same refusal emitted —
// otherwise freezing a `number[]` binding still let writes through
const t: number[] = [1, 2, 3];
Object.freeze(t);
try { t[1] = 99; console.log("typed wrote", t[1]); } catch (e) { console.log("typed write threw", (e as any).constructor.name, t[1]); }
const ts: string[] = ["x", "y"];
Object.freeze(ts);
try { ts[0] = "z"; console.log("typed str wrote", ts[0]); } catch (e) { console.log("typed str threw", (e as any).constructor.name, ts[0]); }

// an unfrozen typed array is untouched in the same shape
const tp: number[] = [1, 2, 3];
tp[1] = 99;
console.log(tp[1], tp.length);

// a frozen plain object was already correct — regression witness
const o: any = { x: 1 };
Object.freeze(o);
try { o.x = 9; console.log("wrote", o.x); } catch (e) { console.log("obj write threw", (e as any).constructor.name, o.x); }
