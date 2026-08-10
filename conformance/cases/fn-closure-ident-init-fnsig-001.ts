// A fn-sig-annotated binding initialized from an IDENT whose own
// binding holds the closure-cell repr must keep the cell repr: the
// stored value is an env cell, and a FnSig (direct-dispatch) slot
// would call_indirect the cell header (EXIT=138 — same family as the
// as-any hole, rotation 357). Keyed on the source binding's SSA repr
// so a real code-address source keeps direct dispatch.

// toplevel
const f = (x: number) => x;
const g: (x: number) => number = f;
console.log(g(7));
console.log(g(7, 9));

// fn-scope mirror
function scope() {
  const f2 = (x: number) => x * 2;
  const g2: (x: number) => number = f2;
  console.log(g2(7));
}
scope();

// capturing source stays correct through the retagged slot
function make(base: number) {
  const add = (x: number) => x + base;
  const h: (x: number) => number = add;
  return h(10);
}
console.log(make(5));
