// Object.entries over an `any[]` array whose elements are immediates
// (numbers, booleans) as well as heap values. The entry builder's value
// slot holds each element's whole boxed value; unboxing it there
// stored a number where a pointer was expected, and reading the entry
// crashed.
const len = 2;
const array = new Array(len).fill(1);
const mixed: any[] = [7, true, "s" + len, { k: 1 }];
const e = Object.entries(array);
console.log(e.length, e[0][0], e[0][1], e[1][1]);
const m = Object.entries(mixed);
console.log(m.map((p: any) => p[0] + ":" + typeof p[1]).join(","), m[2][1], m[3][1].k);
const ta = new Int32Array(array);
console.log(JSON.stringify(Object.entries(ta)), JSON.stringify(Object.values(ta)));
