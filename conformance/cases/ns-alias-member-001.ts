// ns-alias member calls — a top-level immutable alias of a namespace
// singleton dispatches members exactly like the bare namespace name,
// and the alias's VALUE identity stays intact.
const m = Math;
console.log(m.max(1, 2), m.floor(3.7), m.abs(-5));
console.log(m === Math);
const j = JSON;
console.log(j.stringify({ a: 1 }));
const arr: any = j.parse("[1,2]");
console.log(arr[1]);
const c = console;
c.log("via-alias");
c.error("alias-err");
const R = Reflect;
console.log(R.has({ a: 1 }, "a"));
