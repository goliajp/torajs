// Rotation 204 — a reified builtin method cell stored on an object
// (the test262 generic-borrow idiom) dispatches through its MINT
// FAMILY: a String-prototype-minted cell runs the §22.1.3
// ToString(this) generic lane instead of being captured by the
// shared-mid array-like arm (`inst.slice(0,2)` answered `[]`).

// String-family borrow on a plain-object receiver
let inst = { x: 1 };
inst.slice = String.prototype.slice;
console.log(inst.slice(0, 2));
console.log(inst.slice(8));

// custom toString drives the ToString(this) coerce
let named = { toString: () => "hello world" };
named.substring = String.prototype.substring;
console.log(named.substring(6));
named.toUpperCase = String.prototype.toUpperCase;
console.log(named.toUpperCase());

// split borrow
let sp = { toString: () => "a,b,c" };
sp.split = String.prototype.split;
console.log(sp.split(",").length);

// Array-family borrow keeps the array-like generic (family gate
// answers None and falls through unchanged)
let al = { length: 2, 0: "p", 1: "q" };
al.join = Array.prototype.join;
console.log(al.join("-"));

// .call regression guard (the station this fix mirrors)
console.log(String.prototype.slice.call("hello", 1, 3));
