// §21.4.4.45 — Date.prototype[@@toPrimitive] is a real property; the
// hint picks the OrdinaryToPrimitive order, and deleting it makes a
// Date's default hint number order like any other object.
const DP: any = Date.prototype;
const TP = Symbol.toPrimitive;
const f = DP[TP];
console.log(typeof f, TP in DP, f.name, f.length);
const d = Object.getOwnPropertyDescriptor(DP, TP)!;
console.log(d.writable, d.enumerable, d.configurable);
const o = { valueOf() { return "v"; }, toString() { return "s"; } };
console.log(f.call(o, "number"), f.call(o, "string"), f.call(o, "default"));
for (const bad of [[o], [o, "boolean"], [3, "string"]]) {
  try { f.call(...(bad as [any, any])); console.log("no throw"); } catch (e) { console.log((e as Error).constructor === TypeError); }
}
const date = new Date(0);
console.log(typeof (date as any + 1), (date as any) == date.toString());
delete DP[TP];
console.log(TP in DP, (new Date(5) as any) + 1);
