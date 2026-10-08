// an array annotated with an alias of number holds fractional
// elements like a number[] does
type N = number;
const a: N[] = [1, 2.5, 10];
console.log(a, a[1] === 2.5);
let s = 0;
for (const v of a) {
  s += v;
}
console.log(s);
const m: N[][] = [[1.5], [2]];
console.log(m[0][0] + m[1][0], m.length);
const ints: N[] = [1, 2, 3];
ints.push(0.25);
console.log(ints, ints.length);
