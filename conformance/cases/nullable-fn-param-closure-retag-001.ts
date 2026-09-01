// 554-01 — a `(() => number) | null` param carries closure repr like
// any other fn-typed param. The spelling reaching the closure-repr
// pass is `__nullable(__fn()->(number))`, and TWO places tested the
// payload with a bare `starts_with("__fn(")`: the marking entry
// (`is_fnsig_ann`, so the param never entered `fn_params` at all) and
// the rewrite exit (`retag_fn_decls`, which would have left the
// annotation untouched even once marked). With both blind, the value
// arrived as a closure cell while the annotation kept the bare-fn-
// pointer lane, and calling the result `blr`'d the cell's heap header
// — printed here as a raw code address (`2 4300849168`), the bug-327
// family.
//
// Combination-only, as registered: each of `orElse` / `logical` alone
// is EQ, and so is either one called f-first. The failing shape needs
// the FnDecl ternary present AND both nullable-param arrows AND the
// null-first call order.
const f1 = (): number => 1;
const f2 = (): number => 2;

function ternaryDecl(k: number): () => number {
  return k > 0 ? f1 : f2;
}
console.log(ternaryDecl(1)(), ternaryDecl(-1)());

const orElse = (g: (() => number) | null): () => number => g ?? f2;
console.log(orElse(null)(), orElse(f1)());

const logical = (g: (() => number) | null): () => number => g || f2;
console.log(logical(null)(), logical(f1)());
