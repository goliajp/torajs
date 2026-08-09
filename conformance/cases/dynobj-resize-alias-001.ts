// dynobj store split (RFC 20260809): two variables holding the same
// object literal must keep observing each other's writes across the
// growth boundary (7 entries fills the initial dense array; the 8th
// write resizes). Pre-split, only the writing handle was updated to
// the relocated block — the alias kept reading the freed old block.
const a: any = { k1: 1, k2: 2, k3: 3, k4: 4, k5: 5, k6: 6, k7: 7 };
const b: any = a;
b.k8 = 8;
console.log(a.k8);
console.log(a === b);
a.k9 = 9;
console.log(b.k9);
console.log(Object.keys(a).length);
