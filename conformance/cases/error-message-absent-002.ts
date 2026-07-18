// RFC 20260718-error-message-own-prop 刀 2 — own-absent `message`
// (no-arg construction / delete detach) + prototype-chain read-through
// (`Err.prototype.message` shadow → `__proto_Error`'s spec "").
const e1 = new TypeError();
console.log(e1.hasOwnProperty("message"), e1.message, typeof e1.message);
console.log(e1.toString());
const e2: any = new RangeError("x");
delete e2.message;
console.log(e2.message);
class Err extends TypeError {}
(Err.prototype as any).message = "custom-type-error";
const e3 = new Err("has own");
console.log(e3.message);
const e4 = new Err();
console.log(e4.hasOwnProperty("message"), e4.message);
console.log(Object.getOwnPropertyDescriptor(e4, "message") === undefined);
