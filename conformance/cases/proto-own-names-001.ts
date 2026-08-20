// A builtin prototype's methods are own properties. They used to be
// answered one key at a time (hasOwnProperty / gOPD / a read) with no
// enumeration behind them, so getOwnPropertyNames walked the real
// dict, found it empty, and disagreed with hasOwnProperty about the
// same fact.
//
// What is asserted is that agreement, plus the spec-required members
// being present — NOT the prototype's total inventory. Printing the
// full own-name list used to look like stronger coverage, but it
// coupled a stable claim to an unstable one: the oracle's inventory
// grows whenever the oracle ships a new builtin, and this file went
// red the first time `Date.prototype` learned `toTemporalInstant`
// with nothing about tr having changed. Names tr is missing belong in
// the roadmap, where a gap can be tracked; a gate fixture that must
// stay green cannot also be an inventory diff against a moving
// target.
function d(label: string, o: any, required: string[]): void {
  const n: any[] = Object.getOwnPropertyNames(o);
  // Every own name answers the same way through all three faces.
  let agree = true;
  for (const k of n) {
    if (!Object.prototype.hasOwnProperty.call(o, k)) agree = false;
    if (Object.getOwnPropertyDescriptor(o, k) === undefined) agree = false;
  }
  // The spec-required members are there, and are OWN (not inherited).
  const missing: string[] = [];
  for (const k of required) {
    if (n.indexOf(k) < 0 || !Object.prototype.hasOwnProperty.call(o, k)) {
      missing.push(k);
    }
  }
  console.log(
    label + " agree=" + agree + " keys=" + Object.keys(o).length +
      " missing=" + (missing.length === 0 ? "-" : missing.sort().join(","))
  );
}

d("Object.p", Object.prototype, ["constructor", "hasOwnProperty", "isPrototypeOf", "propertyIsEnumerable", "toLocaleString", "toString", "valueOf"]);
d("Array.p", Array.prototype, ["at", "concat", "constructor", "filter", "forEach", "indexOf", "join", "map", "pop", "push", "reduce", "reverse", "slice", "sort", "splice", "toString"]);
d("Number.p", Number.prototype, ["constructor", "toExponential", "toFixed", "toLocaleString", "toPrecision", "toString", "valueOf"]);
d("Boolean.p", Boolean.prototype, ["constructor", "toString", "valueOf"]);
d("BigInt.p", BigInt.prototype, ["constructor", "toLocaleString", "toString", "valueOf"]);
d("Promise.p", Promise.prototype, ["catch", "constructor", "finally", "then"]);
d("Set.p", Set.prototype, ["add", "clear", "constructor", "delete", "entries", "forEach", "has", "keys", "values"]);
d("WeakSet.p", WeakSet.prototype, ["add", "constructor", "delete", "has"]);
d("Function.p", Function.prototype, ["apply", "bind", "call", "constructor", "toString"]);
d("Date.p", Date.prototype, ["constructor", "getDate", "getDay", "getFullYear", "getHours", "getTime", "getTimezoneOffset", "setDate", "setTime", "toISOString", "toJSON", "toString", "valueOf"]);

// the two faces of one fact now agree
console.log(Object.getOwnPropertyNames(Set.prototype).indexOf("add") >= 0, Set.prototype.hasOwnProperty("add"));
console.log(Object.getOwnPropertyNames(Number.prototype).indexOf("toFixed") >= 0, Number.prototype.hasOwnProperty("toFixed"));

// Reflect.ownKeys sees the same set
const ok: any[] = Reflect.ownKeys(Number.prototype);
console.log(ok.length, ok.map((x: any) => String(x)).sort().join(","));

// a monkey-patch writes a real entry over a synthesized name -- it
// must be listed once, not twice
const SP: any = Set.prototype;
SP.add = function (this: any, v: any) { return this; };
const patched: any[] = Object.getOwnPropertyNames(SP);
// The count that matters is how many times "add" appears, not how
// many members Set.prototype has today (ES2025 is still adding them).
console.log(patched.filter((x: any) => x === "add").length, SP.hasOwnProperty("add"));

// a delete tombstone drops the synthesized name from both faces
const WS: any = WeakSet.prototype;
delete WS.add;
const after: any[] = Object.getOwnPropertyNames(WS);
console.log(after.filter((x: any) => x === "add").length, WS.hasOwnProperty("add"));

// nothing else moved: ordinary objects, instances, arrays, strings
const o: any = { a: 1, b: 2 };
console.log(Object.getOwnPropertyNames(o).join(","));
console.log(Object.getOwnPropertyNames([1, 2]).join(","));
console.log(Object.getOwnPropertyNames("ab").join(","));
class C { x = 1; m(): number { return 2; } }
console.log(Object.getOwnPropertyNames(new C()).join(","));
console.log(Object.getOwnPropertyNames(C.prototype).sort().join(","));
