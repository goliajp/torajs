// Index writes through a cast any view of a typed array: a write past
// the end leaves holes, a different kind replaces the element, and
// typed reads of the original binding see both.
const c: number[] = [3, 1, 2];
(c as any)[5] = "x";
console.log(c.length, c[0], c[5]);
console.log(c[4]);
console.log(c);

const d: number[] = [1, 2, 3];
(d as any)[0] = { k: 1 };
(d as any)[2] = "three";
console.log(d.length, d[1]);
console.log(d);

const e: number[] = [4, 5];
(e as any)[1] = 0.25;
console.log(e[0] + e[1]);
