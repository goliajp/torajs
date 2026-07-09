// chunk 737 — an IMMUTABLE top-level binding read by a named fn AND
// captured by a closure promotes to a global slot (pre-fix: the
// closure_captured gate kept it main-local and the named-fn read was
// a loud unknown-identifier). The closure-construction capture
// filter resolves the name to the global, so the lifted body reads
// through GlobalRef — one home, no env copy. Covers arrow / named-fn
// / str inits; a mutable captured binding keeps the env-copy home.
const add = (a: number, b: number): number => a + b;
function useAdd(): number {
  return add(3, 4);
}
const wrapAdd = (): number => add(1, 2);
console.log(useAdd());
console.log(wrapAdd());
const mul: (a: number, b: number) => number = (a: number, b: number): number => a * b;
function useMul(): number {
  return mul(3, 4);
}
const wrapMul = (): number => mul(5, 6);
console.log(useMul());
console.log(wrapMul());
function base(a: number, b: number): number {
  return a - b;
}
const op: (a: number, b: number) => number = base;
function useOp(): number {
  return op(9, 4);
}
const wrapOp = (): number => op(9, 2);
console.log(useOp());
console.log(wrapOp());
const label = "tag";
function readLabel(): string {
  return label + "!";
}
const wrapLabel = (): string => label + "?";
console.log(readLabel());
console.log(wrapLabel());
let counter = 0;
const bump = (): number => {
  counter = counter + 1;
  return counter;
};
console.log(bump());
console.log(counter);
