// RFC 20260720-splice-insert knife 3 — toSpliced (ES2023 §23.1.3.42)
// with the `...items` tail: the immutable sibling clones, splices the
// items into the clone, and leaves the source untouched. Matrix:
// replace / pure-insert / grow past cap / any item into number[] /
// string elems (rc lane: clone + inserted stakes both settle).
const src: number[] = [1, 2, 3];
const outA = src.toSpliced(1, 1, 9);
console.log(src, outA);

const outB = src.toSpliced(1, 0, 7, 8);
console.log(src, outB);

const outC = src.toSpliced(3, 0, 4, 5, 6);
console.log(src, outC);

const a: any = 42;
const outD = src.toSpliced(0, 2, a);
console.log(src, outD);

const strs: string[] = ["a", "b", "c"];
const outE = strs.toSpliced(1, 1, "x", "y");
console.log(strs, outE);
