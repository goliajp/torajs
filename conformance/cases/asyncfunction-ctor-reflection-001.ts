// RFC 20260721-builtin-method-reflection 刀 4 — %AsyncFunction%
// constructor reflection: an async-form cell's `.constructor`
// answers the interned AsyncFunction ctor (name / length /
// prototype faces); plain fns and arrows keep %Function%.
const AF: any = (async function foo() {}).constructor;
console.log(typeof AF);
console.log(AF.name);
console.log(AF.length);
console.log(typeof AF.prototype);
const afArrow: any = (async () => {}).constructor;
console.log(afArrow.name);
const plainCtor: any = (function () {}).constructor;
console.log(plainCtor.name);
const arrowCtor: any = (() => {}).constructor;
console.log(arrowCtor.name);
const AF2: any = (async function () {}).constructor;
if (AF === AF2) {
  console.log("af-identity-ok");
} else {
  console.log("af-identity-BAD");
}
