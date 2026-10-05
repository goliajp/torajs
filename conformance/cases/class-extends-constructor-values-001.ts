// §15.7.14 step 5: a class may extend any value IsConstructor answers
// true for — a builtin constructor read as a value, a bound
// constructor, a Proxy over a function — and an arrow is refused
class S extends Symbol {}
console.log(typeof S);
try {
  new S();
  console.log("constructed");
} catch (e: any) {
  console.log("new S", e instanceof TypeError);
}
const bound: any = function () {}.bind(null);
bound.prototype = {};
class C extends bound {}
console.log(Object.getPrototypeOf(new C()) === C.prototype);
class N extends Number {}
const n: any = new N(5);
console.log(n + 1, n instanceof N);
const arrow: any = () => 1;
try {
  class X extends arrow {}
  console.log("no", typeof X);
} catch (e: any) {
  console.log("arrow", e instanceof TypeError);
}
const P: any = new Proxy(function () {}, {});
class Q extends P {}
console.log(typeof Q);
class B extends BigInt {}
try {
  new B(1);
} catch (e: any) {
  console.log("new B", e instanceof TypeError);
}
