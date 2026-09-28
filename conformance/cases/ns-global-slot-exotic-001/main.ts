// a namespace object read from a named function lives in a global
// slot; it keeps its §10.4.6 exotic behavior there: writes throw,
// Reflect.set answers false, the binding stays live
import * as ns from "./lib";
function f() { return ns; }
try { (ns as any).count = 99; console.log("no throw"); } catch (e: any) { console.log("throw", e.constructor.name, ns.count); }
console.log(Reflect.set(ns, "count", 7), ns.count, f() === ns);
ns.bump();
console.log(f().count, Object.isFrozen(ns), Object.isExtensible(ns));
