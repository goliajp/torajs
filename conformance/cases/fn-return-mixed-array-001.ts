// Heterogeneous array literals in un-annotated return position
// (rotation 291, L3b r290 #2): the implicit-generics return
// inference stamped the FIRST element's type (`["use", x]` →
// string[]) while the literal itself lowers as Arr<Any> — the
// named-fn lane read back NULL (silent wrong), the closure lane
// threw "array element does not match". Mixed literals now infer
// `any[]` (the checker's own anchor-widening answer); uniform
// literals keep their element type.

function mixedLit() {
  return ["use", 1];
}
console.log(mixedLit());

function mixedParam(x: any) {
  return ["use", x];
}
console.log(mixedParam(1));

function viaLocal(x: any) {
  const r = ["use", x];
  return r;
}
console.log(viaLocal(2));

const viaClosure = (x: any) => ["use", x];
console.log(viaClosure(3));

const viaAnyBinding: any = (x: any) => ["lit", x];
console.log(viaAnyBinding(4));

// uniform literals keep the tight element type
function uniformNum() {
  return [1, 2, 3];
}
console.log(uniformNum());
function uniformStr() {
  return ["a", "b"];
}
console.log(uniformStr());
