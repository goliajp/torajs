// A monomorph specialization body discards its diagnostics, so a
// member read the checker rejects (`.q` absent from the inferred
// Struct) lowers to the runtime [[Get]] probe and answers undefined.
// The `=== undefined` beside it must still carry ANY_UNDEF: the
// checker records a type only for expressions it typed successfully,
// and the failing left operand used to skip the right operand's visit
// entirely, leaving the `undefined` literal untyped and packed with
// the NULL tag.
function isMissing(o) { return o.q === undefined; }
console.log(isMissing({ x: 9 }));
console.log(isMissing({ q: 1 }));
console.log(isMissing({ q: undefined }));

function isPresent(o) { return o.q !== undefined; }
console.log(isPresent({ x: 9 }));
console.log(isPresent({ q: 1 }));

// `null` on the same site keeps its own tag.
function isNull(o) { return o.q === null; }
console.log(isNull({ x: 9 }));
console.log(isNull({ q: null }));

// The reversed spelling always worked (the left operand is typed
// before the right one fails) — it stays correct.
function isMissingRev(o) { return undefined === o.q; }
console.log(isMissingRev({ x: 9 }));

// Destructuring defaults are this comparison after desugaring:
// `let q = src.q === undefined ? 2 : src.q`.
function withDefault({ q = 2 }) { return q; }
console.log(withDefault({ x: 9 }));
console.log(withDefault({}));
console.log(withDefault({ q: 7 }));
console.log(withDefault({ q: undefined }));

function nestedDefault({ a: { b = 3 } }) { return b; }
console.log(nestedDefault({ a: { z: 1 } }));

function restDefault({ q = 4, ...rest }) { return q + Object.keys(rest).length; }
console.log(restDefault({ x: 1, y: 2 }));
