// tsc, strict or not, types a mutable `let x = null` / `let x =
// undefined` as `any`: the initializer carries no information. tr
// typed the binding Null / Undefined from its init and rejected the
// first rebind that reached it from a nested function body (the
// scope-correct widen walk starts every fn body fresh) or that was
// an `undefined` init at all (test262 for-await-of dstr `let thisValue
// = null; … return() { thisValue = this }`, rotation 651). A nullish
// init now widens whenever the name is rebound to anything other than
// null / undefined, anywhere in the program.
let thisValue = null;
let count = null;
let iterator = {
  return(a: number, b: number) {
    thisValue = this;
    count = a + b;
    return {};
  },
};
iterator.return(1, 2);
console.log(thisValue === iterator, count);

let u = undefined;
u = 3;
console.log(u + 1);

function f() {
  let l = undefined;
  l = { b: 2 };
  return l.b;
}
console.log(f());

var v = null;
function setV(x: number) {
  v = x * 2;
}
setV(21);
console.log(v);

// Rebound only to null again: nothing to widen for.
let n = null;
n = null;
console.log(n === null, typeof n);

// Never rebound: stays what it was.
const k = null;
console.log(k === null);
