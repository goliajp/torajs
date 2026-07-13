// RFC 20260713-loose-eq-substrate blade 3 — same-type object pairs
// compare by identity; object × primitive walks ToPrimitive.

// same-type struct pair: identity
const o1 = { x: 1 };
const o2 = { x: 1 };
console.log(o1 == o2);
console.log(o1 == o1);
console.log(o1 != o2);

// struct with valueOf × primitives (§7.2.14 steps 11-12)
const v = { valueOf: function () { return 7; } };
console.log(v == 7);
console.log(7 == v);
console.log(v == 8);
console.log(v == true);
console.log(v == "7");

// struct with toString (default valueOf answers the object itself)
const t = { toString: function () { return "hi"; } };
console.log(t == "hi");
console.log(t == "ho");

// plain struct → "[object Object]"
const p = { x: 1 };
console.log(p == "[object Object]");

// array × primitive (join-based ToPrimitive)
const a1 = [1];
console.log(a1 == 1);
console.log(a1 == "1");
const a0: number[] = [];
console.log(a0 == 0);
console.log(a0 == "");
console.log(a0 == false);

// same-type array / date identity
const b1 = [1];
console.log(a1 == b1);
console.log(a1 == a1);
const d = new Date(0);
console.log(d == d);
