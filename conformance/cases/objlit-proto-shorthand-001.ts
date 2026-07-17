// §B.3.1 — a property-shorthand `__proto__` is an ordinary own data
// property; only the `__proto__: v` production sets [[Prototype]].
// Simulation-slot key separation: the internal [[Prototype]] link
// lives under a user-unspellable key (leading-NUL), so an own
// `__proto__` data entry and a real proto link no longer conflate —
// the member read answers the data (§10.1.8.1 own-first),
// hasOwnProperty answers false on a pure proto link, and own-keys
// lists the data entry while hiding the link.
var __proto__ = 2;
var obj = { __proto__, __proto__ };
console.log(obj.hasOwnProperty("__proto__"));
console.log(obj.__proto__ === 2);
console.log(Object.keys(obj).join(","));
const parent = { p: 7 };
const child: any = { __proto__: parent };
console.log(child.p);
console.log(child.hasOwnProperty("__proto__"));
console.log(Object.keys(child).join(","));
console.log(child.__proto__ === parent);
