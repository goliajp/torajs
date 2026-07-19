// fn-expr `this` through a variable-routed accessor face declared in
// a NESTED scope (RFC 20260717-fnexpr-this-channel knife 2,
// nested-decl profile): the const-init lookup recurses through fn
// bodies and blocks, so a face resolved against a function-local
// binding promotes the same way a top-level one does
function getterScope(): any {
  const o: any = {};
  const g = function () { return this._v * 2; };
  Object.defineProperty(o, "x", { get: g });
  o._v = 21;
  return o.x;
}
console.log(getterScope());

function setterScope(): any {
  const o: any = {};
  const s = function (v: any) { this._y = v + 1; };
  o.__defineSetter__("y", s);
  o.y = 9;
  return o._y;
}
console.log(setterScope());

function propsScope(): any {
  const o: any = {};
  const h = function () { return this._w ?? "unset"; };
  Object.defineProperties(o, { w: { get: h } });
  const before = o.w;
  o._w = "set";
  return before + "/" + o.w;
}
console.log(propsScope());

// block-scope decl resolves through the same recursion
{
  const bo: any = {};
  const bg = function () { return this._b + 100; };
  bo.__defineGetter__("b", bg);
  bo._b = 5;
  console.log(bo.b);
}

// a this-free nested fn-expr face keeps the plain closure ABI
function plainScope(): any {
  const o: any = {};
  const five = function () { return 5; };
  Object.defineProperty(o, "five", { get: five });
  return o.five;
}
console.log(plainScope());
