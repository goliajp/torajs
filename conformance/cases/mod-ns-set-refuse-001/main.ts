// §10.4.6.9 — [[Set]] on a module namespace returns false for EVERY
// key, and that is the half no attribute can express: an export is
// `{ writable: true, configurable: false }`, so an ordinary receiver
// carrying exactly those attributes would accept the assignment. The
// refusal belongs to the receiver's identity, which is why it is a
// header bit rather than a property flag.
//
// `b` is the interesting one — it is `export let`, a genuinely mutable
// binding. The module that owns it can still reassign it; what §10.4.6.9
// denies is writing THROUGH the namespace.
//
// Module code is strict, so §13.15.2 turns the false into a throw;
// §28.1.13 Reflect.set hands back the boolean instead.
import * as ns from "./lib";
const n: any = ns;

try { n.a = 9; console.log("no throw", n.a); }
catch (e: any) { console.log("throw", e.constructor.name, n.a); }
try { n.b = 9; console.log("no throw", n.b); }
catch (e: any) { console.log("throw", e.constructor.name, n.b); }
try { n.fresh = 1; console.log("no throw", n.fresh); }
catch (e: any) { console.log("throw", e.constructor.name); }

console.log(Reflect.set(n, "a", 9), n.a);
console.log(Reflect.set(n, "fresh", 1));

// §10.4.6.10 — deleting an export fails off the per-entry seal
// (already shipped); a key that was never there succeeds vacuously.
console.log(Reflect.deleteProperty(n, "a"), Reflect.deleteProperty(n, "nope"));
console.log(n.a, n.b);
