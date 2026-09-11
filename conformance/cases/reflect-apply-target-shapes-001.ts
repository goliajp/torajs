// The target of Reflect.apply is whatever IsCallable admits, not just
// a function expression: a namespace static, a prototype method read
// off its holder, a bound function, a builtin taken as a value. Each
// reaches the callee through the one dispatch
// `method_call_closure_apply_like::apply_list` runs, so each gets the
// thisArg on the receiver channel.
//
// Locked in after rotation 632 admitted the target as a
// receiver-honoring position — these are the shapes the change had to
// keep working, measured against bun before and after.
const probes: [string, () => any][] = [
  ["ns-static", () => Reflect.apply(Math.floor, undefined, [1.75])],
  ["str-proto-method", () => Reflect.apply("".charAt, "abc", [1])],
  ["static-of-ctor", () => Reflect.apply(String.fromCharCode, null, [104, 105])],
  ["bound-fnexpr", () => Reflect.apply(function () { return this; }.bind(Math), undefined, [])],
  ["bound-proto-method", () => Reflect.apply(Array.prototype.concat.bind([1, 2], [3]), undefined, [[4]])],
  ["regexp-proto-exec", () => Reflect.apply(RegExp.prototype.exec, /b/, ["abc"])],
  ["object-proto-toString", () => Reflect.apply(Object.prototype.toString, [], [])],
  ["arrow-target", () => Reflect.apply(() => 11, null, [])],
];
for (const [name, f] of probes) {
  try {
    console.log(name, String(f()));
  } catch (e) {
    console.log(name, "THREW", (e as any).constructor.name);
  }
}
