// A face-mismatched store spelled as a FUNCTION LITERAL.
//
// The mismatch census read a bare Ident rhs (`slot = gb`) only. By
// the time it runs, `lift_arrow_fns` has moved a function literal's
// body to a top-level FnDecl and left a closure naming it, so the
// literal spelling of the same store was invisible — and the bare
// indirect call, shaped by the SLOT's annotation, let the callee
// read an argument register the caller never filled.
//
// The lifted body is closure-shaped (an `__env` first param), which
// is exactly why the forwarder pass's own signature snapshot drops
// it; its user face is that list minus the hidden params.

function ga() { console.log("ga"); }

// Assign site.
let slot: () => void = ga;
slot();
slot = function (p = 5) { console.log("lit", p); };
slot();

// Init site, same rule.
let seeded: () => void = function (q = 9) { console.log("seeded", q); };
seeded();

// A sig-EXACT literal keeps the bare lane — nothing to widen.
let exact: () => void = function () { console.log("exact"); };
exact();
