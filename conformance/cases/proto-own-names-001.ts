// A builtin prototype's methods are own properties. They used to be
// answered one key at a time (hasOwnProperty / gOPD / a read) with no
// enumeration behind them, so getOwnPropertyNames walked the real
// dict, found it empty, and disagreed with hasOwnProperty about the
// same fact.
function d(label: string, o: any): void {
  const n: any[] = Object.getOwnPropertyNames(o);
  console.log(label + " " + n.length + " keys=" + Object.keys(o).length + " | " + n.slice().sort().join(","));
}

d("Object.p", Object.prototype);
d("Array.p", Array.prototype);
d("Number.p", Number.prototype);
d("Boolean.p", Boolean.prototype);
d("BigInt.p", BigInt.prototype);
d("Promise.p", Promise.prototype);
d("Set.p", Set.prototype);
d("WeakSet.p", WeakSet.prototype);
d("Function.p", Function.prototype);
d("Date.p", Date.prototype);

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
console.log(patched.length, patched.filter((x: any) => x === "add").length);

// a delete tombstone drops the synthesized name from both faces
const WS: any = WeakSet.prototype;
delete WS.add;
const after: any[] = Object.getOwnPropertyNames(WS);
console.log(after.length, after.filter((x: any) => x === "add").length, WS.hasOwnProperty("add"));

// nothing else moved: ordinary objects, instances, arrays, strings
const o: any = { a: 1, b: 2 };
console.log(Object.getOwnPropertyNames(o).join(","));
console.log(Object.getOwnPropertyNames([1, 2]).join(","));
console.log(Object.getOwnPropertyNames("ab").join(","));
class C { x = 1; m(): number { return 2; } }
console.log(Object.getOwnPropertyNames(new C()).join(","));
console.log(Object.getOwnPropertyNames(C.prototype).sort().join(","));
