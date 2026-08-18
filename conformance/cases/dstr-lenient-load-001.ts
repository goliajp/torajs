// §13.15.5.4 GetV — a destructuring pattern field read answers
// undefined for an absent property even WITHOUT a default (absent is
// not an error; only a default changes what replaces undefined).
var vals = { x: 3 };
var { x, y } = vals;
console.log(x, y);

// assignment lane, no defaults
var a, b;
({ a, b } = { a: 1 });
console.log(a, b);

// mixed: default on one slot, none on the other
var { p = 9, q } = { q: 2 };
console.log(p, q);

// for-of lane
for (var { m, n } of [{ m: 1 }]) {
  console.log(m, n);
}

// nested pattern where the inner source lacks the field
var { o: { w } } = { o: { z: 5 } };
console.log(w);
