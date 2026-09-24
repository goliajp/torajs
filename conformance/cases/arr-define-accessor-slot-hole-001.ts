// §10.4.2.1 ArrayDefineOwnProperty over a hole: the index becomes an
// accessor property. A slot hole (`new Array(n)`, a literal elision)
// kept its reserved encoding under the new accessor, so every reader
// that asks the slot first still answered "absent".
function t(a: any, tag: string) {
  Object.defineProperty(a, "1", { get: function () { return 1; }, configurable: true });
  console.log(tag, a[1], 1 in a, a.hasOwnProperty(1), a.indexOf(1), a.includes(1),
    a.every((v: any, i: any) => !(i === 1 && v === 1)), Object.keys(a).join(),
    JSON.stringify(Object.getOwnPropertyDescriptor(a, 1)));
}
t(new Array(3), "newArray");
t([0, , 2], "elision");
const b: any = [0, , 2];
Object.defineProperty(b, "0", {
  get: function () {
    Object.defineProperty(b, "1", { get: function () { return 1; }, configurable: true });
    return 0;
  },
  configurable: true,
});
console.log(b.indexOf(1), b.lastIndexOf(1), b.some((v: any) => v === 1));
