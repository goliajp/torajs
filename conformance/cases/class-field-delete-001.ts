// §10.1.10 [[Delete]] on a class instance's DECLARED field: the field
// is {writable, enumerable, configurable: true}, so `delete` removes it —
// hasOwnProperty false, the read falls through to the prototype chain,
// enumeration skips it, getOwnPropertyDescriptor answers undefined — and
// a later assignment recreates it as a plain data property. This is the
// test262 propertyHelper `isConfigurable` probe over class/elements.
// The dotted any-receiver read `c.a` takes the compile-time class IC
// (a direct slot load keyed by class tag); its Obj-tag compare keeps the
// exotic-field header bit, so a deleted or redefined instance falls to
// the runtime probe that reads the sidecar first.
class C {
  a = 42;
  b;
  10 = "ten";
  m() { return "m"; }
}
function mk(): any { return new C(); }
const c: any = mk();
console.log(delete c.a, Object.prototype.hasOwnProperty.call(c, "a"), c.a, "a" in c);
console.log(Object.keys(c).sort().join(","), JSON.stringify(c));
console.log(Object.getOwnPropertyDescriptor(c, "a"));
console.log(delete c.a, Object.prototype.hasOwnProperty.call(c, "a"));
c.a = 7;
console.log(c.a, Object.prototype.hasOwnProperty.call(c, "a"), Object.keys(c).sort().join(","));
const d = Object.getOwnPropertyDescriptor(c, "a");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(delete c[10], Object.prototype.hasOwnProperty.call(c, "10"), c[10], Object.keys(c).sort().join(","));
console.log(delete c.b, Object.prototype.hasOwnProperty.call(c, "b"), c.b);
// prototype chain shows through a deleted own field
(C.prototype as any).a = "proto";
delete c.a;
console.log(c.a, Object.prototype.hasOwnProperty.call(c, "a"));
c.a = 1;
console.log(c.a, Object.prototype.hasOwnProperty.call(c, "a"));
// defineProperty over a deleted field creates it afresh (missing
// attributes default false)
delete c.a;
Object.defineProperty(c, "a", { value: 5 });
const e = Object.getOwnPropertyDescriptor(c, "a");
console.log(e.value, e.writable, e.enumerable, e.configurable);
try { console.log("deleted", delete c.a); } catch (err) { console.log("nonconfigurable", (err as any) instanceof TypeError); }
// for-in after a delete
const f: any = mk();
delete f.b;
const seen: string[] = [];
for (const k in f) seen.push(k);
console.log(seen.sort().join(","));
// a frozen instance refuses
const g: any = mk();
Object.freeze(g);
try { delete g.a; console.log("no throw"); } catch (err) { console.log((err as any) instanceof TypeError, g.a); }
