// RFC 20260804-fnprops-canonical-cell — a fn's property bag is ONE
// storage whichever spelling touches it: the FnSig static spelling
// (fnprops side table) delegates to the canonical forward cell's
// props slot once the cell mints, and a bag written before the mint
// migrates in. Pre-fix `(PROTO as any).type = v` (FnSig spelling)
// and a proto-chain / any-lane read through the cell answered
// different storage (S13.2.2_A1 family: m.type read undefined).
function PROTO(this: any) {}
(PROTO as any).type = "monster";
const p: any = PROTO;
console.log(p.type);
const viaChain: any = {};
Object.setPrototypeOf(viaChain, p);
console.log(viaChain.type);
console.log(Object.getPrototypeOf(viaChain) === p);

function FACTORY(this: any) {}
FACTORY.prototype = PROTO;
console.log(typeof (FACTORY as any).prototype);
const m: any = new (FACTORY as any)();
console.log(m.type);
console.log((PROTO as any).isPrototypeOf(m));

// cell-spelling write visible through the FnSig-spelling read path
p.tail = "wag";
console.log((PROTO as any).tail);
