// §16.2.1.6 — a module's free names resolve in its own environment
// record. A declaration another module made is not visible: the read
// throws ReferenceError, `typeof` answers "undefined" and a write
// throws too, where the flat top level the resolver builds would have
// resolved the name. Both directions: the library reading the entry's
// exported function / let / unexported const, and the entry reading a
// library declaration it did not import.
import { probe, results } from "./lib.ts";
export function A() { return 77; }
export let L = 1;
const hidden = 5;
console.log(probe(), JSON.stringify(results));
console.log(typeof libOnly, typeof A, A());
try { libOnly; } catch (e) { console.log(e.name); }
try { libOnly = 3; } catch (e) { console.log(e.name); }
console.log(L, hidden);
