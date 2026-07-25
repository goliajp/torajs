// A Symbol is a real property key (§6.1.7), not a name spelled
// "Symbol(desc)".
//
// Before this chunk tr had no symbol-keyed property slot at all: two
// separate manglings both landed in the STRING key namespace and did
// not even agree with each other — an object-literal computed key
// folded `Symbol.replace` into the literal name
// `__sym_Symbol_replace__` at parse time, while
// `Object.defineProperty(o, sym, …)` ran the symbol through ToString
// and stored `"Symbol(Symbol.replace)"`. Consequences, all observable
// here: `getOwnPropertySymbols` answered `[]` for every object,
// `getOwnPropertyNames` / `Object.keys` / `for-in` /
// `JSON.stringify` leaked the mangled name, and two distinct symbols
// sharing a description collided into one slot.
//
// The dynobj key slot now holds either key kind — both are 8-aligned
// heap cells, so the pointed-to cell's own `type_tag` discriminates.
// String keys hash and compare by content; symbol keys by cell
// identity (§20.4 — each `Symbol(desc)` allocates fresh, the
// description is not the identity). §10.1.11.1's three own-key
// buckets are split at the source: `iter_order` answers indices then
// strings, `iter_symbol_order` the symbols, so every string-key
// surface is correct by construction.
//
// §7.1.19 ToPropertyKey step 2 ("If key is a Symbol, return key") is
// why the key never reaches step 3's ToString — which §7.1.17 makes a
// TypeError for symbols anyway.

const s1 = Symbol("alpha");
const s2 = Symbol("alpha");
const o: any = { plain: 1 };
Object.defineProperty(o, s1, {
  value: 10,
  enumerable: true,
  writable: true,
  configurable: true,
});
Object.defineProperty(o, s2, { value: 20, enumerable: true });
Object.defineProperty(o, Symbol.replace, { value: 30 });

// three distinct keys — same description does NOT collide
console.log("count", Object.getOwnPropertySymbols(o).length);
console.log("list", Object.getOwnPropertySymbols(o));

// string-key surfaces never see a symbol key
console.log("names", JSON.stringify(Object.getOwnPropertyNames(o)));
console.log("keys", JSON.stringify(Object.keys(o)));
console.log("values", JSON.stringify(Object.values(o)));
console.log("json", JSON.stringify(o));
let seen = "";
for (const k in o) {
  seen += k + ";";
}
console.log("forin", seen);

// descriptors read back per key, attributes included
const d1: any = Object.getOwnPropertyDescriptor(o, s1);
console.log("d1", d1.value, d1.enumerable, d1.writable, d1.configurable);
// s2 took the §6.2.5 defaults for the attributes it omitted
const d2: any = Object.getOwnPropertyDescriptor(o, s2);
console.log("d2", d2.value, d2.enumerable, d2.writable, d2.configurable);
const d3: any = Object.getOwnPropertyDescriptor(o, Symbol.replace);
console.log("d3", d3.value, d3.enumerable, d3.writable, d3.configurable);
// a symbol that was never a key of `o` — including one that shares a
// description with a key that IS present
console.log("miss", Object.getOwnPropertyDescriptor(o, Symbol("alpha")));

// the symbol bucket keeps definition order
const ord: any = {};
const a1 = Symbol("first");
const a2 = Symbol("second");
const a3 = Symbol("third");
Object.defineProperty(ord, a3, { value: 3 });
Object.defineProperty(ord, a1, { value: 1 });
Object.defineProperty(ord, a2, { value: 2 });
console.log("order", Object.getOwnPropertySymbols(ord));

// an object whose only keys are symbols is empty to every string face
const only: any = {};
Object.defineProperty(only, s1, { value: 1, enumerable: true });
console.log("only-json", JSON.stringify(only));
console.log("only-keys", JSON.stringify(Object.keys(only)));
console.log("only-names", JSON.stringify(Object.getOwnPropertyNames(only)));
console.log("only-syms", Object.getOwnPropertySymbols(only).length);

// receivers that carry their property dict in an in-layout slot
const fn: any = function g() {};
Object.defineProperty(fn, s1, { value: 77, enumerable: true });
console.log("fn", Object.getOwnPropertySymbols(fn).length);
console.log("fn-d", Object.getOwnPropertyDescriptor(fn, s1).value);
console.log("fn-name", fn.name);

const arr: any = [1, 2];
Object.defineProperty(arr, s2, { value: 88 });
console.log("arr", Object.getOwnPropertySymbols(arr).length);
console.log("arr-d", Object.getOwnPropertyDescriptor(arr, s2).value);
console.log("arr-json", JSON.stringify(arr), arr.length);
console.log("arr-names", JSON.stringify(Object.getOwnPropertyNames(arr)));

// no symbol keys at all
console.log("bare", Object.getOwnPropertySymbols({ x: 1 }).length);
console.log("bare-arr", Object.getOwnPropertySymbols([1]).length);
