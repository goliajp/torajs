// `setPrototypeOf` first asks whether the prototype is already the
// requested one; that read is an owned reference and is handed back on
// every path — function receivers, exotic receivers, subclassed exotics.
function mk(): any {
  const f: any = function () {};
  const p = { a: 1 }, q = { b: 2 };
  Object.setPrototypeOf(f, p);
  Object.setPrototypeOf(f, p);
  Reflect.setPrototypeOf(f, q);
  f.__proto__ = q;
  return [f, p, q]
}
class Sub extends Array<number> {}
let ok = 0;
for (let i = 0; i < 300; i++) {
  const [f, p, q] = mk();
  if (Object.getPrototypeOf(f) === q && f.b === 2 && f.a === undefined) ok++;
  const s: any = new Sub();
  if (Reflect.setPrototypeOf(s, Sub.prototype)) ok++;
  const arr: any = [1, 2];
  if (Reflect.setPrototypeOf(arr, Array.prototype)) ok++;
  const frozen: any = Object.freeze([1]);
  if (!Reflect.setPrototypeOf(frozen, p) && Reflect.setPrototypeOf(frozen, Array.prototype)) ok++;
}
console.log(ok);
async function* ag() { yield 1 }
const it: any = ag();
console.log(typeof it.next, Object.getPrototypeOf(Object.getPrototypeOf(it)) === Object.getPrototypeOf(ag.prototype));
