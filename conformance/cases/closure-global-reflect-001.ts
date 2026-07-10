// RFC 20260709-closure-global O3 记档销案 — typeof / .name / .length
// on closure-typed top-level globals, pinned after the chunk-798
// registry rewire covered the global binding position for free
// (the runtime fn-addr registry is position-agnostic).
const add = (a: number, b: number): number => a + b;
function useIt(): void {
  console.log(typeof add);
  console.log(add.name);
  console.log(add.length);
}
useIt();
console.log(typeof add);
console.log(add.name);
console.log(add.length);
let mutFn = (x: number): number => x * 2;
console.log(mutFn.name);
console.log(typeof mutFn);
mutFn = (x: number): number => x * 3;
console.log(mutFn.name);
console.log(add(1, 2));
console.log(mutFn(4));
