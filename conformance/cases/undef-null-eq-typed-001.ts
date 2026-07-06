// chunk 612 — nullish equality folds are TYPE-based, not operand-shape
// based: an Undefined/Null-typed binding is a Load (not ConstPtrNull),
// and the `undefined` literal also lowers to a zero ptr (pre-fix it was
// mistaken for `null`, folding `y === undefined` to false).
let y = undefined;
console.log(y === undefined, y === null, y !== undefined, y);
let w;
console.log(w === undefined, w == null, w != null, typeof w);
const n = null;
console.log(y === n, y !== n, n === null, n === undefined);
console.log(y == n, n == y, y == null, n == undefined);
let v = undefined;
console.log(y === v, v == y);
console.log(null === undefined, undefined === null, undefined === undefined, null !== undefined);
function f() {
  let x = undefined;
  const g = () => {
    console.log(x === undefined, x === null, x == null, x);
  };
  g();
}
f();
const a: any = undefined;
console.log(a == null, a === undefined);
const b: any = null;
console.log(b == null, b === null, b === undefined);
