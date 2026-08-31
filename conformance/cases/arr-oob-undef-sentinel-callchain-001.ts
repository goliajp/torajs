// The fall-through table — which functions can answer the `undefined`
// sentinel — and its mirror, which parameters a call site can hand
// one, feed each other, and each was computed once in one order. So
// the table did not compose: a call to a function that is itself on
// it read as an ordinary value one hop out.
const zs: number[] = [1, 2, 3];
function g(): number { return zs[9]; }
function h1(): number { return g(); }
function h2(): number { return h1(); }
function h3(): number { return h2(); }
console.log("chain", h3(), typeof h3());

// declared before the callee it depends on — order must not matter
function up1(): number { return up2(); }
function up2(): number { return zs[9]; }
console.log("fwd", up1());

// param taint fed by a call that is itself only on the table
// because of another call
function takes(p: number): number { return p; }
console.log("param", takes(h3()), typeof takes(h3()));

// a fall-through body reached through two hops
function ft(): number { }
function ft1(): number { return ft(); }
function ft2(): number { return ft1(); }
console.log("ft", ft2());

// an arrow bound to a name, called through the binding
const arrow = (): number => zs[9];
function viaArrow(): number { return arrow(); }
console.log("arrow", viaArrow());

// controls: ordinary values stay ordinary all the way out
function ok(): number { return zs[1]; }
function ok2(): number { return ok(); }
console.log("ok", ok2(), typeof ok2(), ok2() + 1);
