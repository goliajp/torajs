// r359 — the remaining init shapes feeding a fn-sig-annotated slot
// with a closure CELL (siblings of the r358 ident / struct-field
// arms): a call whose callee's ret repr is Closure, a ternary and a
// nullish whose arms both yield cells. Each used to call_indirect
// the cell header (EXIT=138).
function mk(): (x: number) => number {
  const base = 1;
  return (x: number): number => x + base;
}
const g1: (x: number) => number = mk();
console.log(g1(1));

const a = (x: number): number => x + 10;
const b = (x: number): number => x + 20;
const cond: boolean = true;
const g2: (x: number) => number = cond ? a : b;
console.log(g2(1));
const g2b: (x: number) => number = cond ? a : b;
console.log(g2b(2, 99));

let maybe: ((x: number) => number) | null = null;
const g3: (x: number) => number = maybe ?? b;
console.log(g3(1));
