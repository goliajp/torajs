// A namespace-static alias captured by a closure is still that static:
// call / apply keep the thisArg and .length reads the spec length.
const orig = Promise.resolve;
const g = () => orig.call(Promise, 8);
g().then((v: any) => console.log("arrow call", v));
const h = function (x: number) {
  return orig.apply(Promise, [x]);
};
h(2).then((v: any) => console.log("fn expr apply", v));
const nested = () => () => orig.call(Promise, 11);
nested()().then((v: any) => console.log("nested", v));
const mx = Math.max;
const len = () => mx.length;
console.log("length", len());
const bad = () => {
  try {
    orig.call(undefined, 1);
  } catch (e) {
    console.log("no receiver", e instanceof TypeError);
  }
};
bad();
function outer() {
  const rj = Promise.reject;
  const run = (why: string) => rj.call(Promise, why);
  return run("inner");
}
outer().catch((e: any) => console.log("captured in fn", e));
const shadowed = () => {
  const orig = (a: number, b: number) => a + b;
  return orig.call(null, 1, 2) + orig.length;
};
console.log("shadowed", shadowed());
