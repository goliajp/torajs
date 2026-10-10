// The only writes of an `any` value into an object type's elements are
// appends. A fresh object the cast rebuilds as a cell keeps its number
// fields as the runtime values they are, fractions included.

type T = { x: number; y: number };

const xs: T[] = [];
xs.push(JSON.parse('{"x":8.5,"y":2}'));
console.log(xs[0].x, xs[0].y, xs[0].x + xs[0].y);

const ys: Array<T> = [];
ys.unshift(JSON.parse('{"x":1.25,"y":-3}'));
ys.unshift(JSON.parse('{"x":4,"y":0.5}'));
console.log(ys[0].x, ys[0].y, ys[1].x, ys[1].y);

function load(texts: string[]): { n: number }[] {
  const out: { n: number }[] = [];
  for (const t of texts) out.push(JSON.parse(t));
  return out;
}
const ns = load(['{"n":0.5}', '{"n":7}', '{"n":-2.75}']);
console.log(ns.length, ns[0].n + ns[1].n + ns[2].n);

class Store {
  rows: T[] = [];
  add(text: string): void {
    this.rows.push(JSON.parse(text));
  }
}
const st = new Store();
st.add('{"x":0.1,"y":0.2}');
console.log(st.rows[0].x, st.rows[0].y);
