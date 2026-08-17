// §16.2.1.6.3 — a transitive DIAMOND is not ambiguous: both star
// chains converge on the same declaring module, so ResolveExport
// answers one binding. Guards the static-resolution check against
// false-positive ambiguity/unresolvable verdicts.
import { v, w } from "./hub.ts";
console.log(v, w);
