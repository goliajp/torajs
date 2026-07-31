// computed-key member-store fn-expr `this` faces: `recv[k] = function () { …this… }`
// joins the knife-2 face set for Index-store targets (symbol keys stay
// Expr::Index — string-literal keys already desugar to Member), and the
// unannotated empty-objlit receiver (`var obj = {}` — dynobj-certain)
// joins the any-recv set. Runtime symbol-keyed dispatch seeds the
// receiver through the FLAG_CLOSURE_RECV_FIRST channel.

// annotated any receiver, symbol-keyed store + direct call
var obj: any = {};
obj[Symbol.match] = function (s: string) {
  const self: any = this;
  return "ann-sym:" + (self === obj) + ":" + s;
};
console.log(obj[Symbol.match]("a"));

// unannotated empty-objlit receiver — string-key and symbol-key stores
var uobj = {};
uobj.m = function (s: string) {
  const self: any = this;
  return "u-str:" + (self === uobj) + ":" + s;
};
uobj[Symbol.match] = function (s: string) {
  const self: any = this;
  return "u-sym:" + (self === uobj) + ":" + s;
};
console.log(uobj.m("b"));
console.log(uobj[Symbol.match]("c"));

// this-free computed store keeps the plain ABI
var p = {};
p[Symbol.match] = function (s: string) {
  return "plain:" + s;
};
console.log(p[Symbol.match]("d"));

// detached read of a promoted face — §10.2.1.2 strict: this = undefined
var d: any = {};
d[Symbol.match] = function () {
  return "detached:" + (this === undefined);
};
const df: any = d[Symbol.match];
console.log(df());
