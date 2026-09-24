// delete on a typed array of pointer elements (Object.keys products,
// struct arrays) removes the index: the slot is released and reads as
// undefined on every lane, typed aliases included.
const o = { p: 1, q: 2 };
const k: string[] = Object.keys(o);
console.log(delete (k as any)[0], (k as any)[0], k[0], k.length, k.hasOwnProperty("0"), 0 in k, k.join("|"));
const names = Object.getOwnPropertyNames({ a: 1, b: 2, c: 3 });
delete (names as any)[1];
console.log(names[1], names.length, JSON.stringify(names), names.indexOf("c"));
type P = { n: number };
let ps: P[] = [{ n: 1 }, { n: 2 }];
delete (ps as any)[0];
console.log(ps[0], ps[1].n, ps.length);
