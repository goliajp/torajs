// a Math method replaced through defineProperty, and one called
// through an alias of Math, still raise into the caller's try
function g(k: number): number {
  return Math.abs(k);
}
const M = Math;
function h(k: number): number {
  return M.floor(k);
}
const k = parseFloat("1.5");
console.log(g(k), h(k));
Object.defineProperty(Math, "abs", {
  value: (x: number) => {
    throw new Error("abs " + x);
  },
});
Object.defineProperty(Math, "floor", {
  value: (x: number) => {
    throw new Error("floor " + x);
  },
});
try {
  console.log(g(k));
} catch (e) {
  console.log("g caught", (e as Error).message);
}
try {
  console.log(h(k));
} catch (e) {
  console.log("h caught", (e as Error).message);
}
console.log("end");
