// static Symbol lane .description — Symbol() answers undefined
// (§20.4.3.2), not null; typeof / strict-eq see the sentinel.
const s1 = Symbol();
console.log(s1.description);
console.log(typeof s1.description);
console.log(s1.description === undefined);
const s2 = Symbol("hi");
console.log(s2.description);
console.log(typeof s2.description);
console.log(s2.description === undefined);
const d = s1.description;
console.log(d);
console.log(typeof d);
const e = s2.description;
console.log(e);
console.log(typeof e);
