// §16.2.1.6.3 "ambiguous" — two `export * from` clauses landing the
// same requested name from DIFFERENT modules make the indirect export
// ambiguous: a dyn-import candidate that touches it REJECTS with a
// SyntaxError (t262 instn-iee-err-ambiguous-import). A transitive
// diamond (two stars converging on ONE final module) is NOT ambiguous
// and must keep resolving.
import('./exp.ts')
  .then(() => console.log("amb resolved (wrong)"))
  .catch((e: any) => console.log("amb caught:", e.name))
  .then(() => import('./exp-dia.ts'))
  .then((ns: any) => console.log("diamond:", ns.x))
  .catch((e: any) => console.log("diamond caught (wrong):", e.name));
