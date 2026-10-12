// Two functions each bind an arrow to the same local name: a call
// through either binding reaches its own body, whatever the other
// one's parameters are.
function mkA(k: number): (y?: number) => any { return (y?: number) => (y === undefined ? 5 : y) + k }

function one(n: number) {
  const inner = (s: string) => s.length + n;
  return inner("abc");
}
function two(n: number) {
  const inner = (f: () => number) => f() + 1;
  return inner(() => n * 2);
}
function three(n: number) {
  const inner = (a: number, f: (y?: number) => number) => f() + a;
  return inner(n, (y?: number) => (y === undefined ? 5 : y) + n);
}
console.log(one(2), two(3), three(4));

function four() {
  const inner = (f: (y?: number) => number) => f() + f(1);
  return inner(mkA(3));
}
function five(n: number) {
  const inner = (f: (y?: number) => number) => f(n);
  return inner(mkA(1)) + inner((y?: number) => (y === undefined ? 0 : y) * 3);
}
console.log(four(), five(4));

// the same name returning a closure in one function and a number in
// another
function six(k: number) {
  const make = (d: number) => (x: number) => x * d + k;
  return make(2)(5);
}
function seven(k: number) {
  const make = (d: number) => d + k;
  return make(2);
}
console.log(six(1), seven(1));
