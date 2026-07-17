// §B.3.1 — a property-shorthand `__proto__` is an ordinary own data
// property; only the `__proto__: v` production sets [[Prototype]].
// (Recorded boundary: reading `obj.__proto__` back and the
// hasOwnProperty answer on a REAL proto link still conflate with the
// internal simulation slot — the slot-key separation is a follow-up.)
var __proto__ = 2;
var obj = { __proto__, __proto__ };
console.log(obj.hasOwnProperty("__proto__"));
const parent = { p: 7 };
const child: any = { __proto__: parent };
console.log(child.p);
