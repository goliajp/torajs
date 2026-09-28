// a namespace object is live at every reflective face, not only at
// the static member read (§10.4.6.8 reads the binding on each [[Get]])
import * as ns from "./lib";
const show = (v: any) => typeof v === "function" ? "[fn]" : typeof v === "object" ? "[ns]" : String(v);
ns.bump();
const k = "count";
console.log(ns.count, ns[k], "count" in ns);
const copy = { ...ns };
console.log(copy.count, Object.values(ns).length, JSON.stringify(ns));
ns.bump();
console.log(ns[k], copy.count, JSON.stringify(ns));
console.log(Object.entries(ns).map(([a, b]) => a + "=" + show(b)).join(";"));
const seen: string[] = [];
for (const key in ns) seen.push(key + ":" + show((ns as any)[key]));
console.log(seen.join(","));
const d = Object.getOwnPropertyDescriptor(ns, "count")!;
console.log(d.value, d.writable, d.enumerable, d.configurable, "get" in d);
try { (ns as any).count = 99; console.log("no throw"); } catch (e: any) { console.log("throw", e.constructor.name, ns.count); }
console.log(Reflect.set(ns, "count", 7), ns.count);
console.log(Reflect.defineProperty(ns, "count", { value: ns.count }), ns.count);
ns.inner.up();
console.log(ns.inner.n, { ...ns.inner }.n, Object.values(ns.inner)[0]);
const dyn = await import("./lib");
ns.bump();
console.log(dyn.count, typeof dyn, { ...dyn }.count, Object.values(dyn).length, dyn === ns);
