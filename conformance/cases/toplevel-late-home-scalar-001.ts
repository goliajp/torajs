// A top-level binding whose initializer is a number or boolean the
// compiler only knows once it has typed it (a member read, an index,
// a conditional, a comparison) is read and written from function
// declarations — including ones declared above it. It used to stay
// local to the program body: `typeof` answered "undefined" and every
// other read threw a ReferenceError.
function bump(): void {
  n = n + 1;
  flag = !flag;
}
function show(): string {
  return n + " " + flag + " " + first + " " + pick + " " + neg;
}

const xs = [3, 4, 5];
let n = xs.length;
let flag = xs.length > 2;
const first = xs[0];
const pick = flag ? 10 : 20;
const neg = -xs[1];
const circle = Math.PI * xs[0] * xs[0];
let h = xs[0] / 2;

function half(): number {
  h = h + 0.25;
  return h;
}

bump();
bump();
console.log(show());
console.log(typeof n, typeof flag, circle.toFixed(3));
console.log(half(), h);
