// a fn-typed return over a value the checker infers `any`
const objs: any = { f: (n: number) => n + 1, g: (n: number) => n * 10 };
function pick(k: number): (n: number) => number {
  const v = k > 0 ? objs.f : objs.g;
  return v;
}
function direct(): (n: number) => number {
  const parsed = JSON.parse('{"k": 1}');
  return parsed.k > 0 ? objs.f : objs.g;
}
function twice(n: number): number { return n * 2; }
function either(k: number): (n: number) => number {
  if (k > 5) return twice;
  const w = objs.g;
  return w;
}
console.log(pick(1)(4), pick(0)(4), direct()(2), either(9)(3), either(1)(3));
const held = pick(1);
console.log(held === objs.f, typeof held);
