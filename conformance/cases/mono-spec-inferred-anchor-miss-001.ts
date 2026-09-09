// A monomorph specialization body's Struct receiver is an INFERRED
// anchor — `function f(o)` declares nothing about `o`, and the shape
// is whatever one call site passed. §10.1.8.1 [[Get]] on an absent
// property answers undefined, which is what the lowering has always
// done; the checker's reject there was discarded, not enforced, and
// the discard is what left these expressions with no recorded type.
function pick(o) { return o.q ? "then" : "else"; }
console.log(pick({ x: 9 }));
console.log(pick({ q: 1 }));

function pickElse(o) { return o.q ? 1 : (o.r === undefined); }
console.log(pickElse({ x: 9 }));

function second(a, b) { return b; }
function viaCall(o) { return second(o.q, o.r === undefined); }
console.log(viaCall({ x: 9 }));

function viaOr(o) { return o.q || (o.r === undefined); }
console.log(viaOr({ x: 9 }));

function viaTemplate(o) { return `${o.q}|${o.r === undefined}`; }
console.log(viaTemplate({ x: 9 }));

function viaArray(o) { return [o.q, o.r === undefined][1]; }
console.log(viaArray({ x: 9 }));

function viaObject(o) { return { a: o.q, b: o.r === undefined }.b; }
console.log(viaObject({ x: 9 }));

function typeofMiss(o) { return typeof o.q; }
console.log(typeofMiss({ x: 9 }));

// The present field keeps its typed answer.
function readPresent(o) { return o.x + 1; }
console.log(readPresent({ x: 9 }));
