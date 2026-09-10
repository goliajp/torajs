// `Reflect.set(arr, "length", v)` is the [[Set]] in its boolean flavor:
// every refusal the length kernel decides — a non-writable length (a
// freeze, a defineProperty lock), a seal's shrink, a shrink stopped by
// a non-configurable index — answers false without a TypeError, while
// an invalid length is a RangeError in both flavors. Before this the
// any-lane kernel had only the throwing spelling, so `Reflect.set` on
// a sealed array's length threw. A strict assignment stopped by a
// non-configurable index lands as far as it can and then throws.
const mk = (f: (xs: any) => void) => { const xs: any = [1, 2, 3]; f(xs); return xs; };
const t = (tag: string, f: (xs: any) => void, v: any) => { const xs = mk(f); try { console.log(tag, Reflect.set(xs, "length", v), xs.length); } catch (e) { console.log(tag, "threw", (e as Error).message); } };
t("plain shrink", () => {}, 1); t("plain grow", () => {}, 5); t("plain same", () => {}, 3);
t("sealed shrink", (xs) => Object.seal(xs), 1); t("sealed grow", (xs) => Object.seal(xs), 5); t("sealed same", (xs) => Object.seal(xs), 3);
t("frozen shrink", (xs) => Object.freeze(xs), 1); t("frozen same", (xs) => Object.freeze(xs), 3); t("frozen grow", (xs) => Object.freeze(xs), 5);
t("locked same", (xs) => Object.defineProperty(xs, "length", { writable: false }), 3); t("locked shrink", (xs) => Object.defineProperty(xs, "length", { writable: false }), 1);
t("preventExt shrink", (xs) => Object.preventExtensions(xs), 1); t("preventExt grow", (xs) => Object.preventExtensions(xs), 5);
t("plain invalid", () => {}, -1); t("plain valueOf", () => {}, { valueOf() { return 2; } }); t("sealed invalid", (xs) => Object.seal(xs), 1.5);
t("nonconf idx shrink", (xs) => Object.defineProperty(xs, "1", { configurable: false }), 0);
const xs: any = [1, 2, 3]; Object.defineProperty(xs, "1", { configurable: false });
try { xs.length = 0; console.log("assign ok", xs.length); } catch (e) { console.log("assign threw", (e as Error).message, xs.length); }
const ys: any = [1, 2, 3]; Object.defineProperty(ys, "1", { configurable: false });
try { Object.defineProperty(ys, "length", { value: 0 }); console.log("define ok", ys.length); } catch (e) { console.log("define threw", (e as Error).message, ys.length); }
