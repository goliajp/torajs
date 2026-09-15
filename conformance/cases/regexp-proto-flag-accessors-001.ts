// §22.2.6 — `RegExp.prototype` owns get-only accessors for each flag and
// for `source`: a RegExp answers its own flag / pattern, the prototype
// itself answers undefined / "(?:)", any other object is a TypeError.
const P: any = RegExp.prototype;
const names = ["hasIndices", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky", "source", "flags"];
const re: any = /a+b/gimsuy;
const other: any = {};
for (const n of names) {
  const d: any = Object.getOwnPropertyDescriptor(P, n);
  let onOther = "";
  try { d.get.call(other); onOther = "no throw"; } catch (e: any) { onOther = e.constructor.name; }
  let onPrim = "";
  try { d.get.call(1); onPrim = "no throw"; } catch (e: any) { onPrim = e.constructor.name; }
  console.log(n, typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length,
    JSON.stringify(d.get.call(re)), JSON.stringify(n === "flags" ? "" : d.get.call(P)), onOther, onPrim);
}
console.log(P.global, P.source, JSON.stringify(Object.getOwnPropertyNames(P).filter((k: string) => names.includes(k)).sort()));
console.log(new RegExp("x", "dv").hasIndices, new RegExp("x", "dv").unicodeSets, /x/.sticky);
const O: any = Object.create(RegExp.prototype);
try { console.log(O.global); } catch (e: any) { console.log("create", e.constructor.name); }
const M: any = Map.prototype;
try { console.log(M.size); } catch (e: any) { console.log("Map.prototype.size", e.constructor.name); }
console.log(JSON.stringify(P.flags), JSON.stringify(Reflect.get(P, "source")), (P as any)["dot" + "All"]);
