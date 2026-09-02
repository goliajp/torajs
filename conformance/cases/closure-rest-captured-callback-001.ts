// 556-02 — a rest-param closure binding captured into a callback body.
// The binding is variadic in the constructing frame (boxed dual entry);
// the capture preamble used to bind it as a plain local, so the call
// took the fixed-prefix CallIndirect and the rest body read its argv
// slots as parameters (exit 139).
const pre = "P";
const tag = (strs: any, ...vals: any[]): string => pre + strs.join("|") + "#" + vals.length;
const sum = (...ns: number[]): number => ns.reduce((a, b) => a + b, 0);
const plain = (a: number, b: number): number => a * 10 + b;

// captured into a map callback (the original repro)
console.log([1, 2].map((n) => tag`n${n}`).join(";"));
// two levels: closure body constructs the callback that captures it
const outer = (k: number): string => [1, 2].map((n) => tag`n${n}${k}`).join(";");
console.log(outer(7));
// a plain rest closure called through the capture at several arities
console.log([0, 1, 2, 3].map((n) => (n === 0 ? sum() : n === 1 ? sum(n) : n === 2 ? sum(n, n) : sum(n, n, n))).join(","));
// a non-rest capture keeps its direct route (control)
console.log([1, 2].map((n) => plain(n, n + 1)).join(","));
// captured alongside other captures, called in a loop body inside the callback
const runner = (xs: number[]): string => {
  let acc = "";
  xs.forEach((x) => {
    for (let i = 0; i < 2; i++) acc += tag`${x}-${i}` + " ";
  });
  return acc.trim();
};
console.log(runner([3, 4]));
// rest closure captured by a closure that is itself returned (escape)
const mk = (): ((n: number) => number) => (n) => sum(n, 1, 2);
console.log(mk()(10));
