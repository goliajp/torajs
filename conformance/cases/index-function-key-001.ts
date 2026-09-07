// §7.1.19 ToPropertyKey does not ask what KIND of object a key is —
// it asks the object. A function answers through the ordinary
// §20.2.3.5 Function.prototype.toString, so `o[() => { }]` reads the
// property whose name is that function's source text.
//
// The literal half already worked: `{ [() => { }]: 1 }` stores under
// the coerced key. Only the READ BACK was refused, which is the whole
// t262 computed-property-name family — 27 cases across object
// literals, class declarations and class expressions, every one of
// them written as "store under a function, then read it back".
//
// Two layers had to agree. The checker admits a Function-typed key
// exactly where it already admits a Struct-typed one (an object key,
// rotation 438) — the same runtime lane, for the same reason. And the
// dispatch judgment had to learn that the four keyed kernels run
// ToPropertyKey INSIDE the runtime: nothing else in a program like
// this emits a `__torajs_closure_*` symbol, so the closure arm was
// stubbed and the read reached the loud reject instead of the key.

const arrow = () => { };

// Store through a computed name, read back through the same function.
const lit = { [arrow]: 1 };
console.log("lit", lit[arrow], lit[String(arrow)]);

// A key that never appears as a computed NAME anywhere — the shape
// that has only the read to keep the closure's toString reachable.
const bare = {};
bare[arrow] = 5;
console.log("bare", bare[arrow], bare[String(arrow)]);

// A miss answers undefined rather than throwing.
const empty = {};
console.log("miss", empty[arrow]);

// A named function expression coerces through the same face.
const named = function g() { };
const two = {};
two[named] = 7;
console.log("named", two[named], Object.keys(two).length);

// A class method under a computed function name, called back.
class C {
  [arrow]() { return 3; }
}
console.log("method", new C()[arrow]());
