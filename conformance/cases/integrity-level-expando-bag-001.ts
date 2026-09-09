// §7.3.14 SetIntegrityLevel reaches every own property. A cell that
// is not itself a dynobj (an array, a function, a class instance, a
// wrapper, a Map) keeps its expando properties in a lazily attached
// dynobj, and `Object.freeze` / `Object.seal` walked only a dynobj
// receiver: an array's `xs[0.5]` / `xs.foo` stayed writable and
// configurable behind a header that said frozen, a new key was
// created on a frozen array, `Reflect.set` answered true and
// `delete` succeeded on a sealed one. Now the walk runs over the
// cell's expando bag, every bag arm shares the one [[Extensible]]
// gate for a NEW key, and an existing entry answers with its own
// writable / configurable bits.
t("freeze new str key", () => { const xs: number[] = [1, 2]; Object.freeze(xs); (xs as any)["foo"] = 9; return [xs, (xs as any).foo, Object.keys(xs)]; });
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", JSON.stringify(f())); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name, (e as Error).message); }
}
class C { a = 1; }
t("freeze new str key via any recv", () => { const xs: any = [1, 2]; Object.freeze(xs); xs["foo"] = 9; return [xs, xs.foo, Object.keys(xs)]; });
t("freeze new expando any[] 0.5", () => { const a: any[] = [1, 2]; Object.freeze(a); a[0.5] = 9; return [a, a[0.5], Object.keys(a)]; });
t("freeze existing expando 0.5", () => { const xs: number[] = [1, 2]; xs[0.5] = 1; Object.freeze(xs); xs[0.5] = 2; return [xs, (xs as any)["0.5"], Object.keys(xs)]; });
t("seal new expando 0.5", () => { const xs: number[] = [1, 2]; Object.seal(xs); xs[0.5] = 9; return [xs, (xs as any)["0.5"], Object.keys(xs)]; });
t("seal existing expando 0.5", () => { const xs: number[] = [1, 2]; xs[0.5] = 1; Object.seal(xs); xs[0.5] = 2; return [xs, (xs as any)["0.5"], Object.keys(xs)]; });
t("preventExt new expando 0.5", () => { const xs: number[] = [1, 2]; Object.preventExtensions(xs); xs[0.5] = 9; return [xs, (xs as any)["0.5"], Object.keys(xs)]; });
t("preventExt existing expando 0.5", () => { const xs: number[] = [1, 2]; xs[0.5] = 1; Object.preventExtensions(xs); xs[0.5] = 2; return [xs, (xs as any)["0.5"], Object.keys(xs)]; });
t("freeze new NaN key", () => { const xs: number[] = [1]; Object.freeze(xs); xs[NaN] = 9; return [xs, Object.keys(xs)]; });
t("freeze str[] 0.5", () => { const ss: string[] = ["a"]; Object.freeze(ss); ss[0.5] = "z"; return [ss, Object.keys(ss)]; });
t("freeze compound existing", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.freeze(xs); xs[0.5] += 4; return [xs, (xs as any)["0.5"]]; });
t("freeze isFrozen with expando", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.freeze(xs); return [Object.isFrozen(xs), Object.getOwnPropertyDescriptor(xs, "0.5")]; });
t("seal isSealed with expando", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.seal(xs); return [Object.isSealed(xs), Object.isFrozen(xs), Object.getOwnPropertyDescriptor(xs, "0.5")]; });
t("freeze Reflect.set new", () => { const xs: number[] = [1]; Object.freeze(xs); return [Reflect.set(xs, "0.5", 9), Object.keys(xs)]; });
t("freeze Reflect.set existing", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.freeze(xs); return [Reflect.set(xs, "0.5", 9), (xs as any)["0.5"]]; });
t("seal Reflect.set existing", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.seal(xs); return [Reflect.set(xs, "0.5", 9), (xs as any)["0.5"]]; });
t("freeze delete expando", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.freeze(xs); return [delete (xs as any)["0.5"], Object.keys(xs)]; });

t("plain new", () => { const o: any = { a: 1 }; Object.freeze(o); o.b = 2; return [o, Object.keys(o)]; });
t("plain existing", () => { const o: any = { a: 1 }; Object.freeze(o); o.a = 2; return [o]; });
t("plain desc", () => { const o: any = { a: 1 }; Object.freeze(o); return Object.getOwnPropertyDescriptor(o, "a"); });
t("closure new", () => { const f: any = function () {}; Object.freeze(f); f.x = 1; return [Object.keys(f)]; });
t("closure existing", () => { const f: any = function () {}; f.x = 1; Object.freeze(f); f.x = 2; return [f.x, Object.getOwnPropertyDescriptor(f, "x")]; });
t("closure seal delete", () => { const f: any = function () {}; f.x = 1; Object.seal(f); return [delete f.x, Object.keys(f)]; });
t("class inst new", () => { const c: any = new C(); Object.freeze(c); c.b = 2; return [Object.keys(c)]; });
t("class inst existing expando", () => { const c: any = new C(); c.b = 1; Object.freeze(c); c.b = 2; return [c.b, Object.getOwnPropertyDescriptor(c, "b")]; });
t("class inst field", () => { const c: any = new C(); Object.freeze(c); c.a = 2; return [c.a, Object.getOwnPropertyDescriptor(c, "a")]; });
t("class inst seal delete expando", () => { const c: any = new C(); c.b = 1; Object.seal(c); return [delete c.b, Object.keys(c)]; });
t("map new", () => { const m: any = new Map(); Object.freeze(m); m.x = 1; return [Object.keys(m)]; });
t("map existing", () => { const m: any = new Map(); m.x = 1; Object.freeze(m); m.x = 2; return [m.x, Object.getOwnPropertyDescriptor(m, "x")]; });
t("map seal delete", () => { const m: any = new Map(); m.x = 1; Object.seal(m); return [delete m.x, Object.keys(m)]; });
t("wrapper new", () => { const n: any = new Number(1); Object.freeze(n); n.x = 1; return [Object.keys(n)]; });
t("wrapper existing", () => { const n: any = new Number(1); n.x = 1; Object.freeze(n); n.x = 2; return [n.x, Object.getOwnPropertyDescriptor(n, "x")]; });
t("date existing", () => { const d: any = new Date(0); d.x = 1; Object.freeze(d); d.x = 2; return [d.x, Object.getOwnPropertyDescriptor(d, "x")]; });
t("regexp existing", () => { const r: any = /a/; r.x = 1; Object.freeze(r); r.x = 2; return [r.x, Object.getOwnPropertyDescriptor(r, "x")]; });
t("arr seal delete expando", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.seal(xs); return [delete (xs as any)["0.5"], Object.keys(xs)]; });
t("arr isFrozen no expando", () => { const xs: number[] = [1]; Object.freeze(xs); return [Object.isFrozen(xs), Object.isSealed(xs), Object.getOwnPropertyDescriptor(xs, "0")]; });
t("arr Reflect.defineProperty frozen expando", () => { const xs: number[] = [1]; xs[0.5] = 1; Object.freeze(xs); return [Reflect.defineProperty(xs, "0.5", { value: 3 }), (xs as any)["0.5"]]; });
t("arr expando then freeze then getOwnPropertyNames", () => { const xs: any = [1]; xs.foo = 1; Object.freeze(xs); return [Object.getOwnPropertyNames(xs), Object.isFrozen(xs)]; });
