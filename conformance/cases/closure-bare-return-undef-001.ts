// bare return in a value-returning fn answers undefined (§10.2.1.4)
var f: any = function(x: any) { if (x === 1) return; return x; };
console.log(typeof f(1), typeof f(2), f(2));
var obj: any = JSON.parse("{\"a\": 1, \"b\": 2}", function(key: any, value: any) {
  if (key === "b") return;
  return value;
});
console.log(obj.a, "b" in obj);
var o2: any = JSON.parse("{\"a\": 1, \"b\": 2}", function(this: any, key: any, value: any) {
  if (key === "a") { Object.defineProperty(this, "b", { configurable: false }); }
  if (key === "b") return;
  return value;
});
console.log(o2.a, o2.b, "b" in o2);
