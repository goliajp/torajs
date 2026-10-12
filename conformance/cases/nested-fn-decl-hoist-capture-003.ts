// A nested function declaration called before its text, from an arrow or
// function-expression body, runs with the body's later bindings still in
// their dead zone (§10.2.11 binds the function, §8.1.1.1 leaves the `let`
// uninitialized until its declaration runs).

const early = (): string => {
  try {
    return String(read());
  } catch (e) {
    return (e as Error).name;
  }
  let later = 5;
  function read(): number { return later; }
};
console.log(early());

const viaPeer = function (): string {
  try {
    return String(outerCall());
  } catch (e) {
    return (e as Error).name;
  }
  const later = 6;
  function outerCall(): number { return innerRead() + 1; }
  function innerRead(): number { return later; }
};
console.log(viaPeer());

// the same shape once the binding has run
const settled = (): number => {
  let later = 7;
  const v = read();
  return v;
  function read(): number { return later; }
};
console.log(settled());

const typeofEarly = (): string => {
  const t = probe();
  let x = 1;
  return t + ":" + x;
  function probe(): string {
    try {
      return typeof x;
    } catch (e) {
      return (e as Error).name;
    }
  }
};
console.log(typeofEarly());

// A declaration nothing above names keeps its slot; one reached early
// moves up alone.
function order(n: number): number {
  const a = first();
  let k = n * 2;
  function first(): number { return 1; }
  function later(): number { return k + n; }
  return a + later();
}
console.log(order(3));

function reached(n: number): number {
  const a = up();
  const k = n + 1;
  function up(): number { return n; }
  function down(): number { return k * 2; }
  return a + down();
}
console.log(reached(4));
