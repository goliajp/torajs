// §10.2.9/§10.2.10 via CreateBuiltinFunction — getOwnPropertyNames
// surfaces the virtual own length/name pair, length first.
function order(o: any): boolean {
  const names = Object.getOwnPropertyNames(o);
  const li = names.indexOf("length");
  return li >= 0 && names.indexOf("name") === li + 1;
}
console.log(order(Function));
console.log(order(Object));
console.log(order(Promise));
console.log(order(Function.prototype));
function userFn(a: number, b: number): number {
  return a + b;
}
console.log(order(userFn));
const userAny: any = userFn;
console.log(Object.keys(userAny).length);
const f2: any = function pick(x: number): number {
  return x;
};
delete f2.name;
const names2 = Object.getOwnPropertyNames(f2);
console.log(names2.indexOf("name"));
console.log(names2.indexOf("length"));
