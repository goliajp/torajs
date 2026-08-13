// §10.1.9.2 OrdinarySetWithOwnDescriptor step 2 -- assigning over an
// existing own data property keeps its attributes. A builtin
// prototype's methods are own properties that live in no dict, so the
// assignment used to take the "create a new property" path and land
// enumerable, putting the patched name into Object.keys and for-in.
const SP: any = Set.prototype;

function desc(o: any, k: string): string {
  const d: any = Object.getOwnPropertyDescriptor(o, k);
  return d === undefined ? "MISSING" : "w=" + d.writable + " e=" + d.enumerable + " c=" + d.configurable;
}

console.log("before", desc(SP, "has"), Object.keys(SP).length);
SP.has = function (this: any, v: any): boolean { return true; };
console.log("after", desc(SP, "has"), Object.keys(SP).length);

// a second assignment over the now-real entry keeps them too
SP.has = function (this: any, v: any): boolean { return false; };
console.log("again", desc(SP, "has"), Object.keys(SP).length);

// for-in over the prototype stays empty
let f = "";
for (const k in SP) f += k + ",";
console.log("forin", f === "" ? "EMPTY" : f);

// a name the prototype does NOT own is an ordinary new property:
// enumerable, and visible to both surfaces
SP.zzz = 1;
console.log("new", desc(SP, "zzz"), Object.keys(SP).length);
let g = "";
for (const k in SP) g += k + ",";
console.log("forin2", g);

// the patch still answers, and the ordinary object case is unmoved
console.log("call", SP.has.call(new Set(), 1));
const o: any = {};
Object.defineProperty(o, "k", { value: 1, writable: true, enumerable: false, configurable: true });
o.k = 2;
console.log("plain", desc(o, "k"), Object.keys(o).length);
const p: any = {};
p.q = 1;
console.log("plain2", desc(p, "q"), Object.keys(p).length);
