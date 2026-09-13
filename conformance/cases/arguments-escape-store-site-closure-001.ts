// An anonymous function expression stored DIRECTLY into a property
// (`obj[name] = function () { … }`) that captures locals and hands
// its `arguments` on as a value (`method.apply(obj, arguments)`):
// every call reaches it through the receiver's boxed entry, which
// carries the real argc / argv — the shape test262's harness uses
// for `observeMethod`. `arguments` used to stay an unknown identifier.
const calls: string[] = [];
function observe(object: any, name: string) {
  const method = object[name];
  object[name] = function () {
    calls.push("call " + name);
    return method.apply(object, arguments);
  };
}
function attachDot(obj: any): void {
  const method = obj.m;
  obj.m = function () { return method.apply(obj, arguments) * 2; };
}
const o: any = { m(a: number, b: number) { return a * 10 + b; }, n(x: number) { return x + 1; } };
observe(o, "n");
console.log("observed", o.n(4), calls.join(";"));
attachDot(o);
console.log("dot", o.m(1, 2));
