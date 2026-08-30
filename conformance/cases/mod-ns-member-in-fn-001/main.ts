// §16.2.2 — a namespace member read from inside a FUNCTION BODY.
//
// The namespace materialized as a synthetic top-level object literal,
// and a lifted fn body reaches an enclosing top-level binding only
// through an `__env` capture the module resolver never wired: every
// `m.<export>` below answered `warning: unknown identifier m` +
// `ssa-lower: member access on non-object Ptr` before the
// direct-connect pass.
//
// The read never needed the object. `m.twice` names an export the
// resolver ALREADY injected as a top-level declaration, so it reads
// that declaration directly — which is also what leaves the object
// itself free to become the §10.4.6 exotic one.
import * as m from "./lib";

function useIt(x: number): number { return m.twice(x) + m.SCALE; }
const arrow = (x: number): number => m.twice(x) - m.SCALE;
function deep(): number { return useIt(1) + m.SCALE; }

console.log(useIt(3));
console.log(arrow(6));
console.log(deep());
console.log(m.default);
console.log(m.twice(m.SCALE));
