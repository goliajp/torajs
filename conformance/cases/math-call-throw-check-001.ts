// Math methods over primitive arguments never raise, but an object
// or symbol argument runs ToNumber, and a patched method runs user
// code: those calls must still reach the caller's try
function f(k: number): number {
  return Math.imul(k, 3) + Math.max(k, 2) + Math.abs(-k);
}
function v(x: any): number {
  return Math.abs(x);
}
function sy(s: symbol): number {
  return Math.abs(s as any);
}
function g(k: number): number {
  return Math.abs(k);
}
const o = {
  valueOf(): number {
    throw new Error("valueOf");
  },
};
const k = parseFloat("1.5");
console.log(f(k));
try {
  console.log(v(o));
} catch (e) {
  console.log("v caught", (e as Error).message);
}
try {
  console.log(sy(Symbol("q")));
} catch (e) {
  console.log("sy caught", e instanceof TypeError);
}
console.log(g(k));
(Math as any).abs = (x: number) => {
  throw new Error("patched " + x);
};
try {
  console.log(g(k));
} catch (e) {
  console.log("g caught", (e as Error).message);
}
console.log("end");
