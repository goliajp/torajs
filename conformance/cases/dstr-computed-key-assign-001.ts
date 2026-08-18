// §13.15.5.4 ComputedPropertyName in destructuring ASSIGNMENT —
// `({ [expr]: target } = src)`. Regression guard: this shape used to
// parse (the objlit cover fold) and then silently assign nothing.
const k = "a";
let v = 0;
({ [k]: v } = { a: 5 });
console.log(v);

// default fires on missing, not on present
let w = 0;
({ [k + "z"]: w = 42 } = { a: 1 });
console.log(w);
({ [k]: w = 42 } = { a: 6 });
console.log(w);

// member target behind a computed key
const box: any = { inner: 0 };
({ [k]: box.inner } = { a: 7 });
console.log(box.inner);

// mixed static + computed, key evaluation in field order
const order: string[] = [];
function key(n: string): string {
  order.push(n);
  return n;
}
let x = 0;
let y = 0;
({ [key("p")]: x, [key("q")]: y } = { p: 8, q: 9 });
console.log(x, y, order.join(","));

// composite key expression
let z = 0;
({ ["x" + "y"]: z } = { xy: 10 });
console.log(z);
