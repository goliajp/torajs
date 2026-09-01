// rotation 553 — the variadic String.concat checker arm admits Any
// arguments the way the arity-1 general-table path always did
// (§22.1.3.5 step 3.b ToStrings every argument; the lower dispatch
// routes an Any actual through the ToString kernel). Before, only the
// spelling differed: `s(1).concat(Object(5))` compiled while
// `s(1).concat(Object(5), "z")` was a type error.
const s = (n: number): string => "v" + n;

console.log(s(1).concat(Object(5), "z"));
console.log("c".concat(Object(5)));
console.log("a".concat(Object("b"), Object(7), "!"));
console.log(s(2).concat("x", Object(true)));

// Undefined stays admitted alongside (step 3.a face).
const u = undefined;
console.log("u".concat(u, "t"));
