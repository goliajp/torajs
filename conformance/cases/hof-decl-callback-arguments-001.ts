// A function DECLARATION passed by value to a callback slot, whose
// body reads `arguments`, sees the whole §23.1.3 list «kValue, k, O»
// — not a list truncated to its declared arity.
function cb(val: any, idx: any) {
  console.log(arguments.length, arguments[1], typeof arguments[2]);
  return true;
}
[5, 6].forEach(cb);
console.log([5, 6].every(cb), [7].some(cb), [8].filter(cb).length, [9].map(cb)[0]);

function none() {
  return arguments[2][arguments[1]] === arguments[0];
}
console.log([11, 12].map(none).join());

let seen = false;
function red(prev: any, cur: any, idx: any) {
  seen = prev === 1 && arguments[3][idx] === cur && arguments.length === 4;
  return 1;
}
[11].reduce(red, 1);
console.log(seen);
seen = false;
[11].reduceRight(red, 1);
console.log(seen);

function kv(v: any, k: any) {
  console.log(arguments.length, typeof arguments[2]);
}
new Map<string, number>([["a", 1]]).forEach(kv);
new Set<number>([4]).forEach(kv);

function cmp(a: any, b: any) {
  return arguments.length === 2 ? a - b : 0;
}
console.log([3, 1, 2].sort(cmp).join());

function fm(v: any) {
  return [arguments.length, arguments[1]];
}
console.log([9, 8].flatMap(fm).join());

const o = { k: 5 };
function t(this: any, v: any) {
  return this.k + arguments.length;
}
console.log([7].map(t, o)[0]);
