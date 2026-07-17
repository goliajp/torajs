// rotation 128 — an Ident accessor face is a borrow of the binding's
// closure ref; the pair must take its own stake, or a reassignment
// frees the closure under the live pair (test262
// 15.2.3.7-6-a-86-1 SIGSEGV shape: rejected redefine + reassigned
// binding + property write through the surviving pair).
let obj = {};
let set_func = function (value) {
  obj.setVerifyHelpProp = value;
};
Object.defineProperty(obj, "foo", { set: set_func, configurable: false });
set_func = function (value) {
  obj.setVerifyHelpProp1 = value;
};
try {
  Object.defineProperties(obj, { foo: { set: set_func } });
  console.log("no-throw");
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
function poke(o: any, name: string, verifyProp: string): boolean {
  o[name] = "unlikelyValue";
  const readProp: string = verifyProp !== "" ? verifyProp : name;
  return o[readProp] === "unlikelyValue";
}
console.log(poke(obj, "foo", "setVerifyHelpProp"));
const oo: any = obj;
oo.foo = "second";
console.log(oo.setVerifyHelpProp);
console.log("done");
