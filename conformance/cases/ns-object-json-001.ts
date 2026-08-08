// RFC 20260801-ns-object-value (JSON extension) — the JSON namespace
// object as a first-class value: thisArg identity, @@toStringTag
// badge, escaped-singleton method calls (parse / stringify with
// reviver / space), detached JSON.parse, and the reflection lengths.
function cb(this: any) {
  return this === JSON;
}
console.log([11].every(cb, JSON));
console.log(Object.prototype.toString.call(JSON));
var j: any = JSON;
console.log(j.parse('{"a":1}').a);
console.log(j.stringify({ b: 2 }));
var o: any = j.parse("[1,2,3]");
console.log(o.length, o[2]);
console.log(typeof JSON);
console.log(j.stringify({ a: [1, 2] }, null, 2));
var r: any = j.parse('{"x":5}', function (k: any, v: any) {
  return typeof v === "number" ? v * 2 : v;
});
console.log(r.x);
console.log(Object.getPrototypeOf(JSON) === Object.prototype);
console.log(Object.keys(JSON).length);
console.log(JSON.parse.length, JSON.stringify.length);
var p = JSON.parse;
var s: any = p("[7]");
console.log(s[0]);
