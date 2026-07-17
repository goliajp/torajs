// RFC 20260718-accessor-reify family — §10.2.4 AddRestrictedFunctionProperties:
// Function.prototype.caller / .arguments are accessor own entries whose four
// faces are the ONE %ThrowTypeError% intrinsic (identity holds), E0 C1,
// and any invocation throws TypeError.
const cd: any = Object.getOwnPropertyDescriptor(Function.prototype, "caller");
const ad: any = Object.getOwnPropertyDescriptor(Function.prototype, "arguments");
console.log("caller-desc", cd !== undefined, "args-desc", ad !== undefined);
// four-way %ThrowTypeError% identity is asserted by test262
// caller-arguments/accessor-properties.js (bun/JSC diverges from spec
// there — mints distinct throwers — so the identity stays out of this
// bun-parity fixture; tr follows the spec).
console.log("get-type", typeof cd.get);
console.log("enum", cd.enumerable, "conf", cd.configurable);
let t1 = "no-throw";
try { cd.get.call(function f(): number { return 1; }); } catch (e: any) { t1 = (e instanceof TypeError) ? "TypeError" : "other"; }
console.log("invoke-throws", t1);
let t2 = "no-throw";
try { ad.set.call({}, 1); } catch (e: any) { t2 = (e instanceof TypeError) ? "TypeError" : "other"; }
console.log("set-throws", t2);
console.log("thrower-len", cd.get.length);
