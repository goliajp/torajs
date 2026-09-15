// §7.3.22 OrdinaryHasInstance — `v instanceof Array` searches v's
// [[GetPrototypeOf]] chain for Array.prototype; the heap shape of v
// does not decide it.
const vals: [string, any][] = [
  ["relinked object", Object.setPrototypeOf({}, Array.prototype)],
  ["proxy of array", new Proxy([], {})],
  ["created map", Object.create(Map.prototype)],
  ["array", [1, 2]],
  ["prototype itself", Array.prototype],
  ["class extends Array", new (class extends Array {})()],
  ["number wrapper", new Number(1)],
  ["primitive number", 1],
  ["long string", "a string long enough to live on the heap for sure"],
  ["function", function () {}],
  ["date", new Date(0)],
];
for (const [label, v] of vals) {
  const x: any = v;
  console.log(label, x instanceof Array, x instanceof Map, x instanceof Number, x instanceof String,
    x instanceof Function, x instanceof Date, x instanceof Object);
}
