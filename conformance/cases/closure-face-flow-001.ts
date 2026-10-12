// An immutable fn-typed binding that took its value's face hands that
// face on: a fn-typed parameter it reaches is called by the value's own
// convention, at top level, inside a function, inside a generic body,
// and through a closure that captures the binding.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }
function caller(h: (y?: number) => number) { return h() }
function caller3(h: (y?: number) => number) { return h(3) }

const g: (y?: number) => number = mk2(4);
console.log(caller(g), caller3(g), g());

function inner(n: number) {
  const g2: (y?: number) => number = mk2(n);
  return caller(g2) + caller3(g2);
}
console.log(inner(5));

function pass<T>(x: T, n: number): number {
  const g3: (y?: number) => number = mk2(n);
  return caller(g3);
}
console.log(pass(0, 6), pass("a", 7));

function viaClosure(n: number) {
  const g4: (y?: number) => number = mk2(n);
  const run = () => caller(g4);
  return run();
}
console.log(viaClosure(8));

// the parameter keeps the value's identity
function same(h: (y?: number) => number, k: (y?: number) => number) { return h === k }
console.log(same(g, g));
