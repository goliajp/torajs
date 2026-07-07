// Chunk 659 — struct-field method calls (FnSig / Closure slots)
// with a BRANCHING argument. Same family as the chunk-656 regex fix:
// the lower snapshotted cur_block before the args, so a ternary arg
// split blocks and the CallIndirect landed in the terminated
// pre-branch block with the merge block's operand — garbage values
// then SIGBUS (smd1 probe: bun 90/yes!, tr 82745754550 + rc 138).

// FnSig-slot method (plain arrow, no captures).
const s = {
  f: (x: number): number => x * 10,
  g: (t: string): string => t + "!",
};
let n = 0;
for (let i = 0; i < 6; i++) {
  n += s.f(i % 2 === 0 ? 1 : 2);
}
console.log(n);
console.log(s.g(n > 0 ? "yes" : "no"));

// Closure-slot method (captures an outer binding).
const base = 100;
const c = {
  h: (x: number): number => base + x,
};
let m = 0;
for (let i = 0; i < 4; i++) {
  m += c.h(i % 2 === 0 ? 1 : 2);
}
console.log(m);

// Literal-arg regression.
console.log(s.f(3), c.h(3));
