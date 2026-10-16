// RegExp.prototype[@@matchAll] answers a lazy %RegExpStringIteratorPrototype%
// iterator: one RegExpExec per next(), its own prototype under
// %Iterator.prototype%, and every abrupt step of the algorithm propagated.
function t(name: string, f: () => any) {
  try { console.log(name, f()); } catch (e: any) { console.log(name, "threw", e.constructor.name); }
}
t("tag", () => Object.prototype.toString.call(/a/g[Symbol.matchAll]("aXa")));
t("spread", () => JSON.stringify([.../a/g[Symbol.matchAll]("aXa")].map((m: any) => [m[0], m.index])));
t("for-of", () => { const out: any[] = []; for (const m of (/./[Symbol.matchAll]("xy") as any)) out.push(m[0]); return out.join(","); });
t("next", () => { const it: any = /b/g[Symbol.matchAll]("abcb"); const a = it.next(); const b = it.next(); const c = it.next(); return [a.value[0], a.value.index, a.done, b.value.index, c.done, c.value].join(" "); });
t("proto chain", () => { const it = /./[Symbol.matchAll]("a"); const p = Object.getPrototypeOf(it); return [p === Object.getPrototypeOf(/./[Symbol.matchAll]("b")), Object.getPrototypeOf(p) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())), typeof p.next, p.next.name, p.next.length, (p as any).return, Object.prototype.hasOwnProperty.call(p, "next")].join(" "); });
t("next foreign", () => { const p = Object.getPrototypeOf(/./[Symbol.matchAll]("a")); return p.next.call({}); });
t("next on create", () => { const o = Object.create(/./[Symbol.matchAll]("a")); return o.next(); });
t("lazy exec", () => { let n = 0; const r: any = /./g; r.exec = function (s: string) { n++; const m: any = ["x"]; m.index = 0; return n < 3 ? m : null; }; RegExp.prototype[Symbol.matchAll].call(r, "abc"); return n; });
t("species throws", () => { const r: any = /./g; r.constructor = { [Symbol.species]: function () { throw new RangeError("s"); } }; return r[Symbol.matchAll]("a"); });
t("tolength throws", () => { const r: any = /./g; r.lastIndex = { valueOf() { throw new RangeError("li"); } }; return r[Symbol.matchAll]("a"); });
t("non-global once", () => JSON.stringify([.../b/[Symbol.matchAll]("abcb")].map((m: any) => m.index)));
t("empty matches", () => JSON.stringify([.../(?:)/gu[Symbol.matchAll]("a\u{1F600}b")].map((m: any) => m.index)));
t("helper map", () => JSON.stringify(((/\d/g[Symbol.matchAll]("1a2") as any).map((m: any) => m[0])).toArray()));
t("string matchAll", () => [..."a1b2".matchAll(/\d/g)].length);
