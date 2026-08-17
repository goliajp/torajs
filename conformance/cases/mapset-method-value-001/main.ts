// rotation 431 — Map / Set receivers join the builtin
// method-VALUE reify families: `m.get` off a typed receiver
// resolves the interned proto mid-cell, so the read, the
// re-dispatching .call, the this-undefined bare call, and the
// not-a-constructor faces all answer bun-equal.
const m = new Map([["k", 7]]);
const g = m.get;
console.log(typeof g, g.call(m, "k"));
const s = new Set([1, 2]);
const h = s.has;
console.log(h.call(s, 2));
try { (h as any)(); console.log("bare no-throw"); } catch (e) { console.log("bare", (e as Error).constructor.name); }
try { new (g as any)(); console.log("ctor no-throw"); } catch (e) { console.log("ctor", (e as Error).constructor.name); }
try { Map.prototype.entries.call(true); } catch (e) { console.log("brand", (e as Error).constructor.name); }
const fe = m.forEach;
fe.call(m, (v: number, k: string) => console.log("fe", k, v));
