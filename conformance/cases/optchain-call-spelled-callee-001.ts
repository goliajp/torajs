// An optional call whose callee is spelled through a comma or a
// conditional short-circuits on the callee value as a whole: a nullish
// callee answers undefined and its arguments never evaluate.
let log: string[] = [];
function arg(s: string) {
  log.push(s);
  return s;
}
function f(this: any, x: any) {
  return "f:" + String(this?.k) + ":" + x;
}
let g: any = undefined;
const o = { k: 1 };
let c = false;

console.log((c ? f : g)?.call(o, arg("a")));
console.log((0, g)?.call(o, arg("b")));
console.log((c ? f : g)?.apply(o, [arg("c")]));
console.log((0, g)?.bind(o, arg("d")));
console.log((c ? f : g)?.(arg("e")));
console.log((0, g)?.(arg("f")));
console.log(log.length);

c = true;
console.log((c ? f : g)?.call(o, arg("g")));
console.log((0, f)?.(arg("h")));
console.log((c ? f : g)?.apply(o, [arg("i")]));
console.log((0, f)?.bind(o, arg("j"))());
console.log((c ? f : g)?.(arg("k")));
console.log(log.join(","));
