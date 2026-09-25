// delete on an element of a number[] / string[] / object[] the prelude
// does not widen to any[] (an annotated binding, a parameter, a returned
// array): the element becomes a hole — F64 sentinel or NULL under a
// shadow mark — and every consumer sees it absent.
const a: number[] = [1, 2, 3];
console.log(delete a[1], a, 1 in a, a[1], a.length);
function f(xs: number[]): boolean {
  return delete xs[0];
}
const xs = [1.5, 2.5, 3.5];
console.log(f(xs), xs, xs.indexOf(2.5), xs.map((x) => x * 2));
function g(): number[] {
  const r: number[] = [];
  for (let i = 0; i < 5; i++) r.push(i * i);
  delete r[2];
  return r;
}
const r = g();
console.log(r, r.reduce((p, c) => p + c, 0), r.filter((x) => x > 0));
const s: string[] = ["x", "y"];
delete s[1];
console.log(s, s.join("-"), JSON.stringify(s));
const o: { k: number }[] = [{ k: 1 }, { k: 2 }];
delete o[0];
console.log(o.length, 0 in o, o.findIndex((e) => e === undefined));
const n: number[] = [3, 1, 2];
delete n[2];
n.sort();
console.log(n);
