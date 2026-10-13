// A function with no return annotation hands back, as it is, what a
// method of an object literal answers when the fn-typed parameter the
// method calls is handed, somewhere, a closure declared to return `any`:
// the call answers that closure's value, a string here, not a number.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }
function mkS(s: string): (y?: number) => any { return (y?: number) => s }
const obj = {
  m(f: (y?: number) => number) { return f() + f(3) },
};
function viaN() { return obj.m(mk2(10)); }
function viaS() { return obj.m(mkS("q")); }
const a = viaN(), b = viaS();
console.log(typeof a + ":" + a, typeof b + ":" + b);
