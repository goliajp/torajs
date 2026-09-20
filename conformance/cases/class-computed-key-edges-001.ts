// Edge shapes of the computed-member machinery this rotation reworked:
// a key whose ToPropertyKey throws, the same key declared twice, a
// computed static field across an extends chain, an accessor pair
// under one computed key, and the descriptors both halves install.
const bad = { toString() { throw new TypeError("boom"); } };
try { class C { static [bad as any] = 1; } } catch (e: any) { console.log("static key throws", e.constructor.name, e.message); }
try { class D { [bad as any]() {} } } catch (e: any) { console.log("method key throws", e.constructor.name, e.message); }
const k = "dup";
class Dup { static [k] = 1; static [k] = 2; }
console.log("dup static", (Dup as any)[k]);
class DupM { [k]() { return "m1"; } [k]() { return "m2"; } }
console.log("dup method", (new DupM() as any)[k]());
const j = "kk";
class B { static [j] = "base"; m() { return "Bm"; } }
class Inh extends B {}
class Own extends B { static [j] = "derived"; }
console.log((Inh as any)[j], (Own as any)[j], new Inh().m());
const a = "acc";
class Acc { get [a]() { return this._v ?? 0; } set [a](v: number) { this._v = v * 2; } _v?: number; }
const c: any = new Acc();
console.log(c[a]); c[a] = 5; console.log(c[a]);
const d = Object.getOwnPropertyDescriptor(Acc.prototype, a)!;
console.log(typeof d.get, typeof d.set, d.enumerable, d.configurable);
class S { static [j] = 1; }
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(S, j)));
class I { [j] = 2; }
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(new I(), j)));
