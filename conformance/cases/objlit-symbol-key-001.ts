// RFC 20260725-getiterator-getmethod 刀 1 — an object literal's
// `[Symbol.<chain>]` key is a real §6.1.7 symbol key.
//
// It used to fold at parse time into a `__sym_<chain>__` NAME. That
// encoding was a dead end — the only readers of `__sym_…` are the
// class-side vtable and fn_table lookups — so the literal reported
// zero own symbols, answered `undefined` for `o[Symbol.iterator]`,
// and leaked a fake string property into `getOwnPropertyNames`.
//
// It is an ordinary computed key now, so §7.1.19 ToPropertyKey hands
// the store a Symbol instead of a stringified name.

// (Typed `symbol`, not `any` — an `any`-typed KEY in an index access
// is a separate recorded gap, orthogonal to how the literal stores.)
const sym = Symbol("mine");

// Data-property form.
const a: any = { x: 1, [Symbol.iterator]: 7 };
console.log(a.x, a[Symbol.iterator]);
console.log(Object.getOwnPropertySymbols(a).length);
console.log(Object.getOwnPropertyNames(a).join("|"));
console.log(Object.keys(a).join("|"));

// Method-shorthand form — the body used to be dropped on the floor
// (parsed with paren/brace balance, then emitted as a `null` stub).
// It parses and lowers for real now.
const b: any = { y: 2, [Symbol.iterator]() { return "ran"; } };
console.log(b.y, typeof b[Symbol.iterator], b[Symbol.iterator]());
console.log(Object.getOwnPropertySymbols(b).length);
console.log(Object.getOwnPropertyNames(b).join("|"));

// A user symbol behaves the same — nothing here is special-cased to
// the well-known chain.
const c: any = { [sym]: "v", z: 3 };
console.log(c[sym], c.z, Object.getOwnPropertySymbols(c).length);

// Symbol and string keys are separate domains: the description does
// not collide with a same-spelled string key.
const d: any = { "Symbol(mine)": "str-keyed", [sym]: "sym-keyed" };
console.log(d["Symbol(mine)"], d[sym]);
console.log(Object.getOwnPropertyNames(d).length, Object.getOwnPropertySymbols(d).length);

// The key expression evaluates in literal position, so its side
// effects fire in field order.
let log = "";
function k1(): symbol { log = log + "k1,"; return Symbol("one"); }
function k2(): symbol { log = log + "k2,"; return Symbol("two"); }
const e: any = { [k1()]: 1, [k2()]: 2 };
console.log(log, Object.getOwnPropertySymbols(e).length);

// `in` / `hasOwnProperty` / `delete` see the literal-born key too.
console.log(Symbol.iterator in a, a.hasOwnProperty(Symbol.iterator));
delete a[Symbol.iterator];
console.log(Symbol.iterator in a, Object.getOwnPropertySymbols(a).length);

// Descriptors: a literal member is a plain writable/enumerable/
// configurable data property, symbol-keyed or not.
const desc: any = Object.getOwnPropertyDescriptor(b, Symbol.iterator);
console.log(typeof desc.value, desc.writable, desc.enumerable, desc.configurable);
