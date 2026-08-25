// Bound-cell discrimination rides the drop-fn brand (rotation 494)
// — every reflection and dispatch face that asks "is this bound"
// must keep answering across plain closures, builtin-method binds,
// and nested binds.
function add(a: any, b: any, c: any) {
  return a + b + c;
}
const b1 = add.bind(null, 1);
console.log(b1.length, b1.name);
console.log(b1(2, 3));
const s: any = "hey";
const up = s.toUpperCase.bind(s);
console.log(up(), up.length, up.name);
const b2 = b1.bind(null, 10);
console.log(b2.length, b2(100));
console.log(add.length, add.name);
const plain = (x: any) => x * 2;
console.log(plain.length, plain.name);
console.log(typeof b1, typeof up);
