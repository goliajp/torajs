// A symbol key reaches a cell's expando bag through its own lane, and
// that lane never asked the receiver's [[Extensible]] bit: on a frozen
// / sealed / non-extensible array, function, class instance or Map,
// `o[sym] = v` created the property. The bag write now shares the
// one gate every name-keyed arm uses; an existing symbol entry
// answers with the writable / configurable bits the freeze walk set.
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", JSON.stringify(f())); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name, (e as Error).message); }
}
const S1 = Symbol("s1");
class C { a = 1; }
t("arr freeze new sym", () => { const xs: any = [1]; Object.freeze(xs); xs[S1] = 1; return 1; });
t("arr seal new sym", () => { const xs: any = [1]; Object.seal(xs); xs[S1] = 1; return 1; });
t("arr preventExt new sym", () => { const xs: any = [1]; Object.preventExtensions(xs); xs[S1] = 1; return 1; });
t("arr freeze existing sym", () => { const xs: any = [1]; xs[S1] = 1; Object.freeze(xs); xs[S1] = 2; return xs[S1]; });
t("arr seal existing sym", () => { const xs: any = [1]; xs[S1] = 1; Object.seal(xs); xs[S1] = 2; return [xs[S1], Object.getOwnPropertySymbols(xs).length]; });
t("arr seal delete sym", () => { const xs: any = [1]; xs[S1] = 1; Object.seal(xs); return [delete xs[S1]]; });
t("arr freeze desc sym", () => { const xs: any = [1]; xs[S1] = 1; Object.freeze(xs); return Object.getOwnPropertyDescriptor(xs, S1); });
t("arr freeze Reflect.set sym", () => { const xs: any = [1]; Object.freeze(xs); return [Reflect.set(xs, S1, 1), Object.getOwnPropertySymbols(xs).length]; });
t("fn freeze new sym", () => { const f: any = function () {}; Object.freeze(f); f[S1] = 1; return 1; });
t("fn seal existing sym", () => { const f: any = function () {}; f[S1] = 1; Object.seal(f); f[S1] = 2; return f[S1]; });
t("fn freeze existing sym", () => { const f: any = function () {}; f[S1] = 1; Object.freeze(f); f[S1] = 2; return f[S1]; });
t("class inst freeze new sym", () => { const c: any = new C(); Object.freeze(c); c[S1] = 1; return 1; });
t("class inst preventExt existing sym", () => { const c: any = new C(); c[S1] = 1; Object.preventExtensions(c); c[S1] = 2; return c[S1]; });
t("map freeze new sym", () => { const m: any = new Map(); Object.freeze(m); m[S1] = 1; return 1; });
t("map seal delete sym", () => { const m: any = new Map(); m[S1] = 1; Object.seal(m); return [delete m[S1]]; });
t("wrapper freeze new sym", () => { const n: any = new Number(1); Object.freeze(n); n[S1] = 1; return 1; });
t("plain freeze new sym", () => { const o: any = {}; Object.freeze(o); o[S1] = 1; return 1; });
t("plain seal existing sym", () => { const o: any = {}; o[S1] = 1; Object.seal(o); o[S1] = 2; return o[S1]; });
t("well-known sym on frozen arr", () => { const xs: any = [1]; Object.freeze(xs); xs[Symbol.iterator] = 1; return 1; });
t("extensible arr sym still writes", () => { const xs: any = [1]; xs[S1] = 5; return [xs[S1], Object.getOwnPropertySymbols(xs).length]; });
