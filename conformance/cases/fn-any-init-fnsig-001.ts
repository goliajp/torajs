// RFC 20260810-indirect-argc-abi L3b #3 — an Any-typed init landing
// in a fn-type-annotated binding keeps the closure CELL repr: the
// FnSig (raw code address) slot dispatched the cell header as code
// (EXIT=138 probe-abi1). Probes: as-any direct init, any-ident init
// (const + let), as-any reassignment, and a value-returning shape.
const g1: () => void = ((): void => {
  console.log("as-any");
}) as any;
g1();
const a2: any = (): void => {
  console.log("any-ident");
};
const g2: () => void = a2;
g2();
let g3: () => void = a2;
g3();
g3 = ((): void => {
  console.log("reassigned");
}) as any;
g3();
const add: (x: number, y: number) => number = ((x: number, y: number): number => x + y) as any;
console.log(add(2, 3));
