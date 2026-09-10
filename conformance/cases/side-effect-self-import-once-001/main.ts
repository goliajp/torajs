// A side-effect import of the entry itself (§16.2.1.5: a module in
// its own graph is evaluated once) — the resolver must not splice a
// second copy of this body in front of the program.
import "./main.ts";
export const once = 1;
console.log("x", once);
