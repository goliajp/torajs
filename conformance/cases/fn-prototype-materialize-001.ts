// RFC 20260721-builtin-method-reflection 刀 9 — plain-fn
// `.prototype` lazy materialization (§10.2.5 MakeConstructor):
// identity-stable, constructor back-ref, enumeration-invisible;
// arrows and async forms own no prototype.
var fun = function () {};
console.log(typeof fun.prototype);
const funAny: any = fun;
console.log(typeof funAny.prototype);
console.log(typeof funAny.prototype.constructor);
if (funAny.prototype.constructor === funAny) {
  console.log("ctor-backref-ok");
} else {
  console.log("ctor-backref-BAD");
}
const p1: any = funAny.prototype;
const p2: any = funAny.prototype;
if (p1 === p2) {
  console.log("identity-ok");
} else {
  console.log("identity-BAD");
}
console.log(Object.keys(funAny).length);
const arrowAny: any = () => {};
console.log(typeof arrowAny.prototype);
const asyncAny: any = async function () {};
console.log(typeof asyncAny.prototype);
function decl() {}
const declAny: any = decl;
console.log(typeof declAny.prototype);
