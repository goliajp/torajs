// A callee minted for its own call is a temp: released after the call
// and on its throw edge alike, whichever arm the call takes.
let k = 3;
let sum = 0;
let caught = 0;
const mk = () => (n: number) => n + k;
for (let i = 0; i < 5000; i++) {
  sum += (() => k)();
  sum += mk()(i);
  sum += ((...a: number[]) => a.length)(1, 2, i);
  try {
    sum += ((): number => { throw new RangeError("iife " + i); })();
  } catch (e) {
    if (e instanceof RangeError) caught++;
  }
}
console.log(sum, caught);

// Object.assign onto a fresh target: a source getter's raise releases
// the target the call minted.
const src: any = {};
Object.defineProperty(src, "g", { get() { throw new TypeError("g"); }, enumerable: true });
let assigned = 0;
for (let i = 0; i < 5000; i++) {
  try { Object.assign({}, { a: i }, src); } catch (e) { if (e instanceof TypeError) assigned++; }
}
console.log(assigned, Object.keys(Object.assign({}, { a: 1 }, { b: 2 })).join(","));
