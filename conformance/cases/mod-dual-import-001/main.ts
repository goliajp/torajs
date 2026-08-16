// §16.2.1.6 — the same module imported through several request
// shapes shares ONE set of bindings. Pre-fix tora's resolver kept a
// per-path ledger of importer-visible names for the NAMED lane only;
// the namespace and side-effect lanes injected whole declarations
// without consulting it, so a second request shape re-declared the
// first's bindings:
//   import { AV } …  then  import * as ns  → redeclaration of `AV`
//   import * as ns …  then  import { AV }  → same, other direction
//   import "./se"  then  import { S1 }     → same, side-effect lane
//
// Fix (r421): every lane records the decl names it injects in the
// path's ledger and skips a name already there. The namespace object
// still claims the FIELD — it references the existing binding.
import { AV } from "./a.ts";
import * as ns from "./a.ts";
import "./se.ts";
import { S1 } from "./se.ts";
console.log(AV);
console.log(ns.AV, ns.fa());
console.log(S1);
