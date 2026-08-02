// §20.2.1.1 with empty bodyText — zero-arg Function() / new Function()
// is an anonymous empty function (no dynamic code involved), desugared
// to `() => {}` by ast_desugar_builtin_new::fn_ctor. Argument-bearing
// forms stay on the loud reject until the eval-family RFC.
const f: any = Function();
console.log(typeof f);
console.log(f());
const g: any = new (Function as any)();
console.log(typeof g, g());
const proto: any = Function();
function FACTORY() {}
(FACTORY as any).prototype = proto;
const obj: any = new (FACTORY as any)();
try {
  obj.call();
  console.log("no-throw");
} catch (e) {
  console.log("threw", (e as any) instanceof TypeError);
}
