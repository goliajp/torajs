// named fn declaration callbacks receive the HOF thisArg as `this`
var objArray = [1, 2, 3];

function pred(val, idx, obj) {
  console.log("pred this===objArray:", this === objArray, "len:", this.length, "val:", val, "idx:", idx);
  return this === objArray;
}
console.log("every:", [11].every(pred, objArray));
console.log("some:", [11, 12].some(pred, objArray));

function finder(v) {
  return v > this.length;
}
console.log("find:", [1, 2, 9].find(finder, objArray));
console.log("findIndex:", [1, 2, 9].findIndex(finder, objArray));

function mapper(v) {
  return v + this.length;
}
console.log("map:", [10, 20].map(mapper, objArray));

function keeper(v) {
  return v % this.length === 0;
}
console.log("filter:", [3, 4, 6].filter(keeper, objArray));

function eacher(v) {
  console.log("forEach:", v * this.length);
}
[7, 8].forEach(eacher, objArray);

// without a thisArg the callback's this stays undefined (strict module)
function loose(v) {
  console.log("no-thisArg typeof this:", typeof this);
  return true;
}
[1].every(loose);

// a direct call keeps plain-call receiver semantics
loose(1);
