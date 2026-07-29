// §7.1.19 step 2 — a symbol key reaches the lookup uncoerced, so an
// array receiver takes it like the `any` and struct receivers already
// do. An absent symbol key is a miss, not a program-wide reject.
const s1 = Symbol("one");
const s2 = Symbol("two");
const a = [10, 20, 30];
console.log(a[s1]);
console.log(a[s2]);
console.log(typeof a[s1]);

// symbols are distinct even with the same description
const dup = Symbol("one");
console.log(a[dup]);

// a well-known symbol key is still a miss on a plain array read
console.log(typeof a[Symbol.iterator]);

// the other key domains keep working alongside
console.log(a["length"]);
console.log(a[1]);
let k: any = "2";
console.log(a[k]);
