// Phase K.2 — real cross-file imports. Pull function / const / class
// from sibling lib.ts; bun resolves the relative path natively, torajs
// inlines lib's exports via modules::resolve_imports before the
// desugar pipeline. The injected decls are indistinguishable from
// same-file decls downstream.
//
// `Pair` is a type alias exported from lib but NOT listed in this
// file's import — K.2 always injects exported type decls (TS itself
// doesn't require type names in the value-import list, and check.rs
// needs the alias to resolve `makePair`'s return-type annotation).

import { add, mul, ZERO, ONE, TAG, makePair, Counter } from "./lib.ts";

function check(): number {
  if (add(2, 3) !== 5) { throw "#1: add"; }
  if (mul(4, 5) !== 20) { throw "#2: mul"; }
  if (add(ZERO, 7) !== 7) { throw "#3: ZERO"; }
  if (add(ONE, 9) !== 10) { throw "#4: ONE"; }
  if (TAG !== "lib") { throw "#5: TAG"; }
  let p: Pair = makePair(11, 22);
  if (p.fst !== 11 || p.snd !== 22) { throw "#6: pair"; }
  let c: Counter = new Counter(40);
  if (c.inc() !== 41) { throw "#7: counter inc"; }
  if (c.inc() !== 42) { throw "#8: counter inc2"; }
  return 0;
}
console.log(check());
