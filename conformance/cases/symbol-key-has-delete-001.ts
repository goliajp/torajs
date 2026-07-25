// `sym in o`, `o.hasOwnProperty(sym)` and `delete o[sym]` all see the
// symbol-keyed slot.
//
// These are the remaining §7.1.19 ToPropertyKey call sites. Each was
// coercing its key with ToString, which §7.1.17 makes a TypeError for a
// Symbol — so `hasOwnProperty(sym)` threw "Cannot convert a symbol to a
// string" on a call that must simply answer true, and `in` / `delete`
// rejected at compile time. Step 2 of ToPropertyKey returns a Symbol
// key untouched; only the other shapes reach step 3's ToString.
//
// The runtime cores were already symbol-capable from the key-domain
// chunk — the entry table hashes and compares by the key cell's own
// tag. What each needed was a gate in front of its NAME-keyed faces:
// the array index domain, `length`, a function's virtual `name`, a
// class prototype's members, builtin-prototype interned methods, ctor
// statics, the delete-tombstone shading. All of those are string-keyed
// by construction and read the key's Str payload, so a symbol key stops
// before them — after the parts that are key-kind agnostic (the own
// entry probe, and for `in` the user [[Prototype]] walk, per §7.3.12 /
// §10.1.10).

const s = Symbol("s");
const s2 = Symbol("s");
const o: any = {};
o[s] = 1;

// same description, different key — `in` and hasOwnProperty must agree
console.log("in", s in o, s2 in o);
console.log("hasOwn-method", o.hasOwnProperty(s), o.hasOwnProperty(s2));
console.log("hasOwn-call", Object.prototype.hasOwnProperty.call(o, s));

// §7.3.12 HasProperty walks the chain; §20.1.3.4 HasOwnProperty does not
const proto: any = {};
proto[s2] = 2;
const child: any = Object.create(proto);
console.log("inherited", s2 in child, child.hasOwnProperty(s2), child[s2]);
const grand: any = Object.create(child);
console.log("grandparent", s2 in grand, grand[s2]);
const bare: any = Object.create(null);
console.log("null-proto", s2 in bare);

// §13.5.1.2 → §10.1.10 OrdinaryDelete
console.log("delete", delete o[s], o[s], s in o);
console.log("delete-count", Object.getOwnPropertySymbols(o).length);
// deleting an absent property is true
console.log("delete-absent", delete o[s2]);
// a non-configurable symbol key refuses (module-strict)
const nc: any = {};
Object.defineProperty(nc, s, { value: 7, configurable: false });
try {
  const r = delete nc[s];
  console.log("delete-nonconf", "no throw", r, nc[s]);
} catch (e: any) {
  console.log("delete-nonconf", "threw", e instanceof TypeError);
}

// string keys and the element domain keep their own answers on the very
// same objects
const mix: any = { a: 1 };
mix[s] = 2;
console.log("mix", "a" in mix, mix.hasOwnProperty("a"), delete mix["a"], "a" in mix, mix[s]);

const fn: any = function g() {};
fn[s] = 3;
console.log("fn", s in fn, fn.hasOwnProperty(s), "name" in fn, fn.hasOwnProperty("name"));
console.log("fn-del", delete fn[s], s in fn, "name" in fn);

const arr: any = [1, 2];
arr[s] = 4;
console.log("arr", s in arr, arr.hasOwnProperty(s), 0 in arr, "length" in arr);
console.log("arr-del", delete arr[s], s in arr, 0 in arr, arr.length);

// a receiver with no dict at all owns no symbol key
console.log("empty", s in {});
const fresh: any = {};
console.log("fresh", s in fresh, fresh.hasOwnProperty(s), delete fresh[s]);
