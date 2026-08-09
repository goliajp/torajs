// RFC 20260808-construct-channel B2 knife (rotation 345) — a
// this-reading fn-expr handed as an argument to a program-local
// FnDecl with explicit `any` params promotes: the value rides the
// any lane, whose every call path honors FLAG_CLOSURE_RECV_FIRST.
// Profile mixes the argument position with a `.call` face and a
// bare direct call (this = undefined per §10.2.1.2 strict).
var captured: any = "init";
var C = function () {
  captured = this;
};
function takesAny(f: any, g: any): boolean {
  return f === g;
}
console.log(takesAny(C, C));
C.call({ tag: 7 });
console.log(captured.tag);
C();
console.log(captured === undefined ? "undef-this" : "leak");
