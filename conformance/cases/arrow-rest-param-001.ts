// cluster #13 (rotation 442): rest parameters on arrow functions —
// the parse_fn wedge's arrow mirror.
const g = (...args: any[]) => args.length;
console.log(g(1, 2));
const h = (...args) => args.length;
console.log(h(5));
const sum = (first: number, ...rest: number[]) => {
  let t = first;
  for (const r of rest) t += r;
  return t;
};
console.log(sum(1, 2, 3, 4));
const fwd = (...xs) => xs;
console.log(fwd("a", "b"));
const none = (...e) => e.length;
console.log(none());
