// An object that reaches the any world hands over the arrays it
// holds: writes through `(o as any).xs` land in `o.xs`.
const o = { xs: [1, 2] };
(o as any).xs.push("s");
console.log(o.xs.length, o.xs[2]);
console.log(o.xs);

class H {
  xs: number[] = [1, 2];
}
const h = new H();
const a: any = h;
a.xs.push("h");
a.xs[0] = null;
console.log(h.xs.length, h.xs[0], h.xs[2]);

function f(p: any): void {
  p.list.unshift(true);
}
const holder = { list: [4.5] };
f(holder);
console.log(holder.list.length, holder.list[0], holder.list[1]);
