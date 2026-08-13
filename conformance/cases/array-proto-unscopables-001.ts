// §23.1.3.44 Array.prototype[@@unscopables] -- a null-prototype object
// whose 16 entries are all `true`, held under a {W:0, E:0, C:1} own
// entry of Array.prototype. tr has no `with`, so nothing observes the
// unscoping itself; what is observable is the property's existence,
// its key order (§10.1.11.1 lists own keys in creation order) and its
// attributes. All of that answered undefined before.
const u = (Array.prototype as any)[Symbol.unscopables];

console.log(typeof u);
console.log(Object.getPrototypeOf(u));
console.log(JSON.stringify(Object.keys(u)));
console.log(u.at, u.flat, u.values, u.includes, u.toSorted);
console.log(u.push, u.pop, u.map);

console.log(JSON.stringify(Object.getOwnPropertyDescriptor(u, "at")));
console.log(
  JSON.stringify(
    Object.getOwnPropertyDescriptor(Array.prototype, Symbol.unscopables),
  ),
);

// @@iterator is created first (§23.1.3.40 before §23.1.3.44), so the
// symbol list must answer in that order.
const syms = Object.getOwnPropertySymbols(Array.prototype);
console.log(syms.length);
console.log(syms[0] === Symbol.iterator, syms[1] === Symbol.unscopables);
