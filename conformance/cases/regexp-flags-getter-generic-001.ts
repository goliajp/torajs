// §22.2.6.4 — `RegExp.prototype.flags` is an accessor whose getter is
// receiver-generic: eight Gets in spec order, ToBoolean'd, throws re-thrown.
var desc: any = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags");
console.log(typeof desc, typeof desc.get, desc.set, desc.enumerable, desc.configurable, desc.get.length, JSON.stringify(desc.get.name));
var get: any = desc.get;
var calls = "";
var re: any = {
  get hasIndices() { calls += "d"; },
  get global() { calls += "g"; },
  get ignoreCase() { calls += "i"; },
  get multiline() { calls += "m"; },
  get dotAll() { calls += "s"; },
  get unicode() { calls += "u"; },
  get unicodeSets() { calls += "v"; },
  get sticky() { calls += "y"; },
};
console.log(JSON.stringify(get.call(re)), calls);
console.log(get.call({ global: 1, sticky: "yes", unicode: 0, multiline: null, dotAll: {} }));
console.log(get.call(/x/dgimsuy), get.call(/x/), get.call(new RegExp("a", "gi")));
console.log(get.call(RegExp.prototype), "flags" in RegExp.prototype, RegExp.prototype.hasOwnProperty("flags"));
for (const v of [undefined, null, 1, "s", true, Symbol()]) {
  try { get.call(v); console.log("no throw"); } catch (e) { console.log("non-object:", typeof v, e instanceof TypeError); }
}
var poisoned: any = { get global() { throw new RangeError("g"); }, get sticky() { throw new RangeError("y"); } };
try { get.call(poisoned); console.log("no throw"); } catch (e) { console.log("rethrow:", (e as any).message); }
console.log(/a/gi.flags, /a/.flags, Object.keys(RegExp.prototype).length);
