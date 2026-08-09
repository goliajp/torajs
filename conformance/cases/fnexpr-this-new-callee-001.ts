// Rotation 345 knife 5 — `new C()` (NewDynamic callee) joins the
// construct-channel use shapes: the plain-fn kernel invokes through
// invoke_with_this with the allocated `this`, which shifts argv on
// FLAG_CLOSURE_RECV_FIRST. Mixed profile: new callee + explicit-any
// argument + bare direct call (this = undefined).
var captured: any = 0;
var C = function () {
  captured = this;
};
var a: any = new C();
console.log(typeof captured, captured === a);
console.log(a instanceof C);
function eats(f: any): boolean {
  return f === f;
}
console.log(eats(C));
C();
console.log(captured === undefined ? "undef-this" : "leak");
