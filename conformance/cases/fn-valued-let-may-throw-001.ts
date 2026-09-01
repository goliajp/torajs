// rotation 552 (551-04) — a call through a fn-valued `let` whose init is
// not a closure literal (a ternary between two arrows, a picked value)
// was invisible to the may-throw fixed point: neither a FnDecl nor a
// lifted-closure alias, so a callback whose only throw source was `f()`
// was judged never-throwing, the HOF loop pruned its check, and the
// pending throw strayed into the next checked call — `for … try {
// a(i).map(x => f()) } catch` caught one in two (300000 / 600000, 50MB).
// Every such call now counts as may-throw; every shape answers what bun
// answers.
const boom = (): any => {
  throw new Error("x");
};
const mk0 = (): any => ({});
const a = (n: number): number[] => [n, n];
let n = 0;
let f = n === 0 ? boom : mk0;
let caught = 0;

for (let i = 0; i < 200; i++) {
  try {
    const r = a(i).map((x: number): any => f());
    if (r.length < 0) n++;
  } catch (e) {
    caught++;
  }
}
console.log("map", caught);

caught = 0;
for (let i = 0; i < 200; i++) {
  try {
    a(i).forEach((x: number): void => {
      f();
    });
  } catch (e) {
    caught++;
  }
}
console.log("forEach", caught);

type Thunk = () => any;
const pick = (k: number): Thunk => (k > 0 ? boom : mk0);
let g = pick(1);
caught = 0;
for (let i = 0; i < 200; i++) {
  try {
    a(i).filter((x: number): boolean => g() === 1);
  } catch (e) {
    caught++;
  }
}
console.log("filter", caught);

f = mk0;
g = pick(0);
console.log("swap", a(1).map((x: number): any => f()).length, a(1).filter((x: number): boolean => g() === 1).length);
