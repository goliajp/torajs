// L3b #17 — ES §10.1.11.1 property order on dynobj iteration faces:
// array-index keys ascending first, then the rest in insertion
// order. Non-canonical numeric strings ("01") stay insertion-order
// string keys.
const o: any = {};
o["b-x"] = 1;
o["10"] = 2;
o["2"] = 3;
o.a = 4;
o["0"] = 5;
o["01"] = 6;
console.log(o);

// nested dynobj value renders with the same ordering
const n: any = {};
n["z-key"] = 9;
n["7"] = 8;
const outer: any = {};
outer.inner = n;
outer["3"] = 0;
console.log(outer);

// interleaved writes keep the integer prefix sorted regardless of
// insertion order (hole exclusion is unit-tested kernel-side —
// `delete` has no surface syntax yet)
const d: any = {};
d["5"] = 50;
d["1"] = 10;
d["9"] = 90;
d.mid = 1;
console.log(d);
console.log("done");
