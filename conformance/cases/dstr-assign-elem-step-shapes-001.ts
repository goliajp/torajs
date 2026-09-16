// The element shapes that ride the stepping lane beside a plain
// target: a default (fired only by undefined), an elision (steps and
// discards), a nested pattern (no reference of its own), a rest
// element (drains from whatever park the prefix left), and a
// past-the-end slot (a done iterator answers undefined).
const o: any = {};
function k(n: string): string {
  return n;
}
[o[k("a")] = 9, , o[k("c")], ...o[k("r")]] = [1, 2, 3, 4, 5];
console.log(o.a, o.c, JSON.stringify(o.r));
const p: any = {};
[p[k("x")], [p[k("y")]]] = [5, [6]];
console.log(p.x, p.y);
const q: any = {};
[q[k("m")] = 7, q[k("n")] = 8] = [];
console.log(q.m, q.n);
const r: any = {};
[r[k("s")] = 1, r[k("t")]] = [undefined];
console.log(r.s, r.t);
