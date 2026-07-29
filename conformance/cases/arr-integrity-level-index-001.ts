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

// a frozen plain object was already correct — regression witness
const o: any = { x: 1 };
Object.freeze(o);
try { o.x = 9; console.log("wrote", o.x); } catch (e) { console.log("obj write threw", (e as any).constructor.name, o.x); }
