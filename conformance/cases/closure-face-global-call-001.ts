// A top-level fn-typed binding whose initializer calls a function
// returning a closure is visible from named function bodies, whether
// the value's face matches the annotation or not, and whether the
// binding is reassigned later.
function mk(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }
function mkA(k: number): (y?: number) => any { return (y?: number) => (y === undefined ? 5 : y) + k }
function mkN(k: number) { return (y?: number): number => (y === undefined ? 5 : y) + k }
function named(y?: number): number { return y === undefined ? 1 : y }

const g: (y?: number) => number = mk(1);
const h: (y?: number) => number = mkA(2);
const n: (y?: number) => number = mkN(3);
let m: (y?: number) => number = mk(3);
const p: (y?: number) => number = named;

function use() {
  return `${g()} ${g(3)} ${h()} ${h(3)} ${n()} ${n(3)} ${m()} ${m(3)} ${p()} ${p(4)}`;
}
console.log(use());
m = mkA(10);
console.log(use());
function setM() { m = mk(20); }
setM();
console.log(use());
console.log(g === g, typeof h, n(1.5) + 0.25);
