// RFC 20260810-arr-sparse-grow 刀 E — a giant `length` grow on an
// any-typed array goes sparse in O(1): reads under the tail answer
// undefined, `in` answers false (implicit holes are not own
// properties), a shrink back inside the materialized extent restores
// the dense cell, and a tail write materializes exactly.
const a: any = [10, 20];
a.length = 4294967295;
console.log(a.length);
console.log(a[100]);
console.log(100 in a);
console.log(0 in a);
a.length = 2;
console.log(a.length);
console.log(a[1]);
a[1] = 99;
console.log(a[1]);

const b: any = [1];
b.length = 100000000;
b[50] = 7;
console.log(b[50]);
console.log(b[49]);
console.log(49 in b);
console.log(50 in b);
console.log(b.length);
