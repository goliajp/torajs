// 1. sync-called self-referencing closure in the initializer
try { const x = (() => x)(); console.log("no", x); } catch (e) { console.log("1", e instanceof ReferenceError); }
try { const o = { v: (() => o)() }; console.log("no", o); } catch (e) { console.log("2", e instanceof ReferenceError); }
function f() { const x: any = (() => x)(); return x; }
try { f(); console.log("no"); } catch (e) { console.log("3", e instanceof ReferenceError); }
{ try { const y = [(() => y)()]; console.log("no", y); } catch (e) { console.log("4", e instanceof ReferenceError); } }
// 2. earlier closure called before the declaration
function g() {
  const read = () => z;
  try { read(); console.log("no"); } catch (e) { console.log("5", e instanceof ReferenceError); }
  let z = 7;
  return read();
}
console.log("6", g());
// 3. inert shapes stay untaxed and still work
const isEven = (n: number): boolean => n === 0 ? true : isOdd(n - 1);
const isOdd = (n: number): boolean => n === 0 ? false : isEven(n - 1);
console.log("7", isEven(10), isOdd(7));
const fib = (n: number): number => n < 2 ? n : fib(n - 1) + fib(n - 2);
console.log("8", fib(20));
const obj = { self: () => obj, n: 3 };
console.log("9", obj.self().n);
// 4. a later closure is not in the dead zone
let later = 1;
const readLater = () => later;
console.log("10", readLater());
// 5. writes through an early closure
function h() {
  const w = () => { q = 2; };
  try { w(); } catch (e) { console.log("11", e instanceof ReferenceError); }
  let q = 1;
  w();
  return q;
}
console.log("12", h());
