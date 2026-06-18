// ES2025 mutating Set setops — union / intersection / difference /
// symmetricDifference per §24.2.3.{15,16,17,18}. Each returns a
// fresh Set; the receiver / argument Sets are not modified.

const a = new Set<number>([1, 2, 3]);
const b = new Set<number>([2, 3, 4]);
const c = new Set<number>([1, 2]);
const empty = new Set<number>();

// union
console.log("--- a.union(b)");
console.log(a.union(b));
console.log("--- a.union(c)");
console.log(a.union(c));
console.log("--- a.union(a)");
console.log(a.union(a));
console.log("--- a.union(empty)");
console.log(a.union(empty));
console.log("--- empty.union(a)");
console.log(empty.union(a));
console.log("--- empty.union(empty)");
console.log(empty.union(empty));

// intersection
console.log("--- a.intersection(b)");
console.log(a.intersection(b));
console.log("--- a.intersection(c)");
console.log(a.intersection(c));
console.log("--- a.intersection(a)");
console.log(a.intersection(a));
console.log("--- a.intersection(empty)");
console.log(a.intersection(empty));
console.log("--- empty.intersection(a)");
console.log(empty.intersection(a));

// difference
console.log("--- a.difference(b)");
console.log(a.difference(b));
console.log("--- a.difference(c)");
console.log(a.difference(c));
console.log("--- a.difference(a)");
console.log(a.difference(a));
console.log("--- a.difference(empty)");
console.log(a.difference(empty));
console.log("--- empty.difference(a)");
console.log(empty.difference(a));

// symmetricDifference
console.log("--- a.symdiff(b)");
console.log(a.symmetricDifference(b));
console.log("--- a.symdiff(c)");
console.log(a.symmetricDifference(c));
console.log("--- a.symdiff(a)");
console.log(a.symmetricDifference(a));
console.log("--- a.symdiff(empty)");
console.log(a.symmetricDifference(empty));
console.log("--- empty.symdiff(a)");
console.log(empty.symmetricDifference(a));

// Originals unchanged after mutating setops
const w = a.union(b);
const x = a.intersection(b);
const y = a.difference(b);
const z = a.symmetricDifference(b);
console.log("a-unchanged", a.size);
console.log("b-unchanged", b.size);

// String keys — heap-key rc bookkeeping
const xa = new Set<string>(["apple", "banana", "cherry"]);
const xb = new Set<string>(["banana", "cherry", "date"]);
console.log("--- str.union");
console.log(xa.union(xb));
console.log("--- str.intersection");
console.log(xa.intersection(xb));
console.log("--- str.difference");
console.log(xa.difference(xb));
console.log("--- str.symdiff");
console.log(xa.symmetricDifference(xb));

// Originals (string) unchanged
console.log("xa-unchanged", xa.size);
console.log("xb-unchanged", xb.size);

// Chain — union then intersection
console.log("--- chain");
console.log(a.union(b).intersection(new Set<number>([2, 4, 99])));
