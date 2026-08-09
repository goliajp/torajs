// Rotation 345 (RFC 20260808-construct-channel) — `o instanceof C`
// where C is a plain-fn VALUE binding: §7.3.22 OrdinaryHasInstance
// against C's canonical .prototype (the same fnprops cell the
// construct kernel links). Covers the manual-chain, plain-object,
// primitive, new-product, and Array.from.call construct-product
// shapes. fn_prototype_pair probes before minting, so every channel
// answers one identity.
var C = function () {};
var o: any = Object.create(C.prototype);
console.log(o instanceof C);
var p: any = { k: 1 };
console.log(p instanceof C);
console.log(5 instanceof C);
var a: any = new C();
var b: any = new C();
console.log(a instanceof C, b instanceof C);
console.log(Object.getPrototypeOf(a) === C.prototype);
var r = Array.from.call(C, []);
console.log(r instanceof C, r.constructor === C);
