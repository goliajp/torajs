// 424-02 — a named import's decl reaches lib-internal top-level
// names (its init calls a non-exported helper). The hidden-dependency
// census injects the closure under deconflict mangles, so the entry's
// own same-named helpers keep their bindings.
import { c, d, e } from "./lib_basic.ts";
function mk() { return 100; }
function h2() { return 200; }
console.log(c, d, e);
console.log(mk(), h2());
