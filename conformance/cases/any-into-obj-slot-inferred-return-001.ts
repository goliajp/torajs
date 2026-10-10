// a function whose return type the compiler inferred from an object
// literal it builds (and then grows through `any` property writes)
// returns that object as it is: only a return type the program wrote
// is checked
function make() {
  var r = {} as any;
  r.desc = { get: function () { return 1; } };
  r.a = Object.defineProperty({}, "prop", r.desc);
  r.info = Object.getOwnPropertyDescriptor(r.a, "prop");
  return r;
}
const r1 = make();
const r2 = make();
console.log(r1.desc.get === r1.info.get, r1.desc.get === r2.desc.get, r1.a.prop);
function lit(flag: boolean): any {
  const o = { x: 1 };
  if (flag) return o;
  return { x: 2, extra: "y" };
}
console.log(lit(true).x, lit(false).extra);
function iterLike() {
  let i = 0;
  return {
    next() {
      i++;
      return i > 2 ? { done: true, value: undefined } : { done: false, value: i };
    },
  };
}
const it = iterLike();
console.log(JSON.stringify([it.next(), it.next(), it.next()]));
