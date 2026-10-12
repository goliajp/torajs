// A fn-typed binding inside a generic function's body, or inside a
// body cloned for a function value of another face, holds a value of
// another face than its annotation: each specialization and each clone
// calls the value by the value's own convention.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

function gen<T>(x: T): T {
  const g: (y?: number) => number = mk2(1);
  console.log(g(), g(3));
  let m: (y?: number) => number = mk2(10);
  m = mk2(20);
  console.log(m(), m(3));
  return x;
}
console.log(gen(1), gen("s"));

function caller(h: (y?: number) => number) {
  const g: (y?: number) => number = mk2(2);
  return h() + g();
}
const f = (y?: number) => (y === undefined ? 5 : y) + 1;
console.log(caller(f), caller(mk2(3)));

