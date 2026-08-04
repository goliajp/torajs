// r295 — fn-value faces the wrap collector missed: a bare fn-Ident
// RHS on a top-FnDecl receiver member store (`FACTORY.prototype =
// PROTO` was a loud FnSig box reject), and an as-cast fn-Ident
// receiver of an any-method call (`(PROTO as any).isPrototypeOf(m)`
// failed to peel the As before the receiver wrap). The construct
// chain then uses the written prototype (S13.2.2_A1 family).
function PROTO(this: any) {}
function FACTORY(this: any) {}
FACTORY.prototype = PROTO;
const m0: any = {};
console.log((PROTO as any).isPrototypeOf(m0));
console.log((PROTO as any).hasOwnProperty("nonexist"));
const m1: any = new (FACTORY as any)();
console.log((PROTO as any).isPrototypeOf(m1));
