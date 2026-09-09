// §7.3.16 TestIntegrityLevel: `Object.isSealed` / `Object.isFrozen`
// ask [[IsExtensible]] and then every own property's descriptor. The
// header markers `Object.seal` / `Object.freeze` leave are the fast
// path; a receiver that was only made non-extensible is answered by
// the walk — an empty object or a Map (no own properties) is sealed
// and frozen, an object whose every property was defined
// {configurable: false, writable: false} is frozen, an array with an
// element is not sealed, and a function is not sealed while its name
// and length stay configurable. Before this isFrozen read the header
// bit alone and isSealed answered false for every non-dynobj cell.
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", JSON.stringify(f())); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name, (e as Error).message); }
}
const sf = (o: any) => [Object.isSealed(o), Object.isFrozen(o)];
class E { a = 1; }
t("primitives", () => [sf(1), sf("s"), sf(true), sf(null), sf(undefined), sf(Symbol("x")), sf(10n)]);
t("extensible shapes", () => [sf({}), sf([]), sf(function () {}), sf(new Map()), sf(new E()), sf(new Number(1))]);
t("preventExt empty obj", () => { const o: any = {}; Object.preventExtensions(o); return sf(o); });
t("preventExt obj with prop", () => { const o: any = { a: 1 }; Object.preventExtensions(o); return sf(o); });
t("preventExt obj all locked", () => { const o: any = {}; Object.defineProperty(o, "a", { value: 1, writable: false, configurable: false }); Object.preventExtensions(o); return sf(o); });
t("preventExt obj nonconf writable", () => { const o: any = {}; Object.defineProperty(o, "a", { value: 1, writable: true, configurable: false }); Object.preventExtensions(o); return sf(o); });
t("preventExt obj accessor nonconf", () => { const o: any = {}; Object.defineProperty(o, "x", { get() { return 1; }, configurable: false }); Object.preventExtensions(o); return sf(o); });
t("preventExt obj symbol locked", () => { const o: any = {}; const s = Symbol("k"); Object.defineProperty(o, s, { value: 1, writable: false, configurable: false }); Object.preventExtensions(o); return sf(o); });
t("preventExt obj symbol loose", () => { const o: any = {}; o[Symbol("k")] = 1; Object.preventExtensions(o); return sf(o); });
t("preventExt empty arr", () => { const xs: number[] = []; Object.preventExtensions(xs); return sf(xs); });
t("preventExt arr with elem", () => { const xs: number[] = [1]; Object.preventExtensions(xs); return sf(xs); });
t("preventExt arr idx locked", () => { const xs: any = [1]; Object.defineProperty(xs, "0", { configurable: false, writable: false }); Object.preventExtensions(xs); return sf(xs); });
t("preventExt arr idx locked len RO", () => { const xs: any = [1]; Object.defineProperty(xs, "0", { configurable: false, writable: false }); Object.defineProperty(xs, "length", { writable: false }); Object.preventExtensions(xs); return sf(xs); });
t("preventExt empty arr len RO", () => { const xs: any = []; Object.defineProperty(xs, "length", { writable: false }); Object.preventExtensions(xs); return sf(xs); });
t("preventExt arr expando", () => { const xs: any = []; xs.foo = 1; Object.preventExtensions(xs); return sf(xs); });
t("preventExt fn", () => { const f: any = function () {}; Object.preventExtensions(f); return sf(f); });
t("preventExt arrow", () => { const f: any = () => 1; Object.preventExtensions(f); return sf(f); });
t("preventExt fn no name len", () => { const f: any = function () {}; Object.preventExtensions(f); delete f.name; delete f.length; return sf(f); });
t("preventExt arrow no name len", () => { const f: any = () => 1; Object.preventExtensions(f); delete f.name; delete f.length; return sf(f); });
t("preventExt map", () => { const m: any = new Map(); Object.preventExtensions(m); return sf(m); });
t("preventExt map expando", () => { const m: any = new Map(); m.x = 1; Object.preventExtensions(m); return sf(m); });
t("preventExt class inst", () => { const e: any = new E(); Object.preventExtensions(e); return sf(e); });
t("preventExt class inst locked", () => { const e: any = new E(); Object.defineProperty(e, "a", { configurable: false, writable: false }); Object.preventExtensions(e); return sf(e); });
t("preventExt number wrapper", () => { const n: any = new Number(1); Object.preventExtensions(n); return sf(n); });
t("preventExt string wrapper", () => { const s: any = new String("ab"); Object.preventExtensions(s); return sf(s); });
t("preventExt date", () => { const d: any = new Date(0); Object.preventExtensions(d); return sf(d); });
t("preventExt regexp", () => { const r: any = /a/g; Object.preventExtensions(r); return sf(r); });
t("sealed shapes", () => { const o: any = { a: 1 }; const xs: any = [1]; const f: any = function () {}; const g: any = () => 1; const m: any = new Map(); const e: any = new E(); Object.seal(o); Object.seal(xs); Object.seal(f); Object.seal(g); Object.seal(m); Object.seal(e); return [sf(o), sf(xs), sf(f), sf(g), sf(m), sf(e)]; });
t("frozen shapes", () => { const o: any = { a: 1 }; const xs: any = [1]; const f: any = function () {}; const m: any = new Map(); const e: any = new E(); Object.freeze(o); Object.freeze(xs); Object.freeze(f); Object.freeze(m); Object.freeze(e); return [sf(o), sf(xs), sf(f), sf(m), sf(e)]; });
t("typed arr lanes", () => { const xs: number[] = [1]; const ys: string[] = []; Object.freeze(xs); Object.preventExtensions(ys); return [Object.isFrozen(xs), Object.isSealed(xs), Object.isFrozen(ys), Object.isSealed(ys)]; });
t("typed fn lane", () => { function h() {} Object.freeze(h); return [Object.isFrozen(h), Object.isSealed(h)]; });
t("typed class inst lane", () => { const e = new E(); Object.seal(e); return [Object.isFrozen(e), Object.isSealed(e)]; });
t("proxy", () => { const target: any = {}; const p = new Proxy(target, {}); Object.preventExtensions(p); return [sf(p), sf(target)]; });
t("proxy with prop", () => { const target: any = { a: 1 }; const p = new Proxy(target, {}); Object.freeze(target); return [sf(p)]; });
t("Object as value", () => { const O: any = Object; const o: any = {}; O.preventExtensions(o); return [O.isSealed(o), O.isFrozen(o), O.isFrozen(1)]; });
