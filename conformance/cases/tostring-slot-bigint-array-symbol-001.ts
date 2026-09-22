// §7.1.17 ToString in the string-method argument slots and in
// property keys: a BigInt is its digits (the `n` suffix is console
// formatting only), an array joins, a Symbol throws a TypeError.
const s = "ab0c12,3";
console.log(s.indexOf(12n, 0), s.lastIndexOf(0n, 99));
console.log("x".padStart(3, 0n as any), "x".padEnd(4, [1, 2] as any));
console.log(s.indexOf([2, 3] as any, 0));
try {
  s.indexOf(Symbol("q") as any, 0);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
const o: any = {};
o[12n] = 1;
console.log(Object.keys(o), Object.hasOwn(o, 12n), 12n in o);
console.log(1n, [2n], "a", 3n);
