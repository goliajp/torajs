// The integrity level reaches the own properties a cell keeps outside
// its bag, and a re-parent asks the receiver: a frozen RegExp's
// lastIndex is non-writable (by assignment and by the Set a global or
// sticky exec / test / replace performs); Object.setPrototypeOf on a
// non-extensible function, array, Map or object refuses a different
// prototype (same value stays a no-op, a cycle is its own refusal);
// Object.seal / freeze of a typed array with elements is a TypeError
// (§10.4.5.3 refuses the non-configurable define) while an empty one
// freezes; and a function's written `prototype` keeps {writable: true,
// enumerable: false, configurable: false}.
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", JSON.stringify(f())); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name, (e as Error).message); }
}
const sf = (o: any) => [Object.isSealed(o), Object.isFrozen(o), Object.isExtensible(o)];
t("fn prototype write desc", () => { const f: any = function () {}; f.prototype = { z: 1 }; const d = Object.getOwnPropertyDescriptor(f, "prototype"); return [d?.writable, d?.enumerable, d?.configurable, Object.keys(f)]; });
t("fn prototype write then delete", () => { const f: any = function () {}; f.prototype = { z: 1 }; return [delete f.prototype]; });
t("typed array freeze empty", () => { const u = new Uint8Array(0); Object.freeze(u); return sf(u); });
t("typed array freeze nonempty", () => { const u = new Uint8Array(2); Object.freeze(u); return sf(u); });
t("typed array seal nonempty", () => { const u = new Uint8Array(2); Object.seal(u); return [sf(u), (u[0] = 5), u[0]]; });
t("typed array preventExt", () => { const u = new Uint8Array(2); Object.preventExtensions(u); return [sf(u), (u[0] = 5), u[0]]; });
t("typed array preventExt expando", () => { const u: any = new Uint8Array(1); Object.preventExtensions(u); u.x = 1; return 1; });
t("arraybuffer freeze", () => { const b = new ArrayBuffer(4); Object.freeze(b); return sf(b); });
t("date freeze then setTime", () => { const d = new Date(0); Object.freeze(d); d.setTime(5); return [d.getTime(), sf(d)]; });
t("map freeze then set", () => { const m = new Map(); Object.freeze(m); m.set(1, 2); return [m.size, sf(m)]; });
t("Array.prototype freeze", () => { const p: any = Array.prototype; return [Object.isFrozen(p), Object.isExtensible(p)]; });
t("frozen string wrapper index write", () => { const s: any = new String("ab"); Object.freeze(s); s[0] = "z"; return 1; });
t("frozen string wrapper expando", () => { const s: any = new String("ab"); Object.freeze(s); s.x = 1; return 1; });
t("symbol wrapper", () => { const b: any = Object(Symbol("q")); Object.freeze(b); b.x = 1; return 1; });
t("frozen promise expando", () => { const p: any = Promise.resolve(1); Object.freeze(p); p.x = 1; return 1; });
t("frozen error message write", () => { const e: any = new Error("m"); Object.freeze(e); e.message = "n"; return e.message; });
t("frozen error new prop", () => { const e: any = new Error("m"); Object.freeze(e); e.code = 1; return 1; });
t("frozen regexp lastIndex", () => { const r: any = /a/g; Object.freeze(r); r.lastIndex = 3; return r.lastIndex; });
t("frozen regexp exec", () => { const r: any = /a/g; Object.freeze(r); return r.exec("a")?.[0]; });
t("frozen class static field write", () => { class K { static s = 1; } Object.freeze(K); (K as any).s = 2; return K.s; });
t("frozen class static method write", () => { class K { static m() { return 1; } } Object.freeze(K); (K as any).m = () => 2; return K.m(); });
t("frozen class prototype method write", () => { class K { m() { return 1; } } Object.freeze(K.prototype); (K.prototype as any).m = () => 2; return new K().m(); });
t("frozen instance method shadow", () => { class K { m() { return 1; } } const k: any = new K(); Object.freeze(k); k.m = () => 2; return k.m(); });
t("frozen getter-only field", () => { const o: any = { get x() { return 1; } }; Object.freeze(o); o.x = 2; return o.x; });
t("frozen obj Object.assign", () => { const o: any = { a: 1 }; Object.freeze(o); Object.assign(o, { b: 2 }); return o; });
t("frozen arr Object.assign", () => { const xs: any = [1]; Object.freeze(xs); Object.assign(xs, { 0: 5 }); return xs; });
t("frozen obj defineProperty new", () => { const o: any = {}; Object.freeze(o); Object.defineProperty(o, "a", { value: 1 }); return 1; });
t("frozen arr defineProperty new idx", () => { const xs: any = [1]; Object.freeze(xs); Object.defineProperty(xs, "1", { value: 1 }); return 1; });
t("frozen arr defineProperty existing same", () => { const xs: any = [1]; Object.freeze(xs); Object.defineProperty(xs, "0", { value: 1 }); return xs; });
t("frozen arr defineProperty existing diff", () => { const xs: any = [1]; Object.freeze(xs); Object.defineProperty(xs, "0", { value: 2 }); return xs; });
t("frozen obj setPrototypeOf", () => { const o: any = {}; Object.freeze(o); Object.setPrototypeOf(o, null); return 1; });
t("frozen obj setPrototypeOf same", () => { const o: any = {}; Object.freeze(o); Object.setPrototypeOf(o, Object.prototype); return 1; });
t("preventExt arr setPrototypeOf", () => { const xs: any = []; Object.preventExtensions(xs); Object.setPrototypeOf(xs, null); return 1; });
t("frozen fn setPrototypeOf", () => { const f: any = function () {}; Object.freeze(f); Object.setPrototypeOf(f, null); return 1; });
t("frozen obj __proto__ write", () => { const o: any = {}; Object.freeze(o); o.__proto__ = null; return 1; });
t("frozen regexp lastIndex desc", () => { const r: any = /a/g; Object.freeze(r); return Object.getOwnPropertyDescriptor(r, "lastIndex"); });
t("frozen regexp non-global exec", () => { const r: any = /a/; Object.freeze(r); return r.exec("a")?.[0]; });
t("frozen regexp test global", () => { const r: any = /a/g; Object.freeze(r); return r.test("a"); });
t("frozen regexp replace global", () => { const r: any = /a/g; Object.freeze(r); return "aa".replace(r, "b"); });
t("frozen regexp replace non-global", () => { const r: any = /a/; Object.freeze(r); return "aa".replace(r, "b"); });
t("sealed regexp lastIndex write", () => { const r: any = /a/g; Object.seal(r); r.lastIndex = 3; return r.lastIndex; });
t("sticky frozen exec", () => { const r: any = /a/y; Object.freeze(r); return r.exec("a"); });
t("cycle", () => { const a: any = {}; const b: any = Object.create(a); Object.setPrototypeOf(a, b); return 1; });
t("preventExt arr same proto", () => { const xs: any = []; Object.preventExtensions(xs); Object.setPrototypeOf(xs, Array.prototype); return 1; });
t("preventExt fn same proto", () => { const f: any = function () {}; Object.preventExtensions(f); Object.setPrototypeOf(f, Function.prototype); return 1; });
t("preventExt map null", () => { const m: any = new Map(); Object.preventExtensions(m); Object.setPrototypeOf(m, null); return 1; });
t("Reflect.setPrototypeOf preventExt", () => { const o: any = {}; Object.preventExtensions(o); return [Reflect.setPrototypeOf(o, null), Reflect.setPrototypeOf(o, Object.prototype)]; });
t("Reflect.setPrototypeOf frozen fn", () => { const f: any = function () {}; Object.freeze(f); return [Reflect.setPrototypeOf(f, null), Reflect.setPrototypeOf(f, Function.prototype)]; });
t("typed preventExt integrity", () => { const u = new Uint8Array(1); Object.preventExtensions(u); return [sf(u), Reflect.ownKeys(u), Object.getOwnPropertyDescriptor(u, "0")]; });
t("typed empty seal", () => { const u = new Uint8Array(0); Object.seal(u); return sf(u); });
