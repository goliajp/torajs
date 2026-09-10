// An EMPTY array literal assigned into a typed array slot mints its
// block at the slot's layout. `lower_empty` otherwise answers an
// any-flavored block whose ArrId is not the slot's, and the coercion
// check refused the store — the shape every ES5-era
// Array/prototype/* test writes (`x = []; x[0] = 0; x[3] = 3`).
//
// An empty block holds no element, so there is no element type to
// disagree about; a non-empty literal of a different element type
// keeps its loud reject.
const x = new Array(0, 1, 2, 3);
console.log(x.join());
let y = [1, 2];
y = [];
y[0] = 0;
y[3] = 3;
console.log(y.join(), y.length);
y = [7];
console.log(y.join());
y = [];
console.log(y.length, y.join());
y.push(9);
console.log(y.join());
let s = ["a", "b"];
s = [];
s[1] = "c";
console.log(s.join(), s.length);
