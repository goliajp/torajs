// dynamic spread into a ctor (chunk 684): new-expr args ride the
// spread-aware call-arg parser; desugar rewrites the New into the
// __new_<C> factory call, whose fixed arity drives the existing
// guarded index-read expansion.
class P {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  sum(): number { return this.x + this.y; }
}
const arr: number[] = [40, 2];
const p = new P(...arr);
console.log(p.sum());
// prefix + spread
const arr1: number[] = [5];
const q = new P(37, ...arr1);
console.log(q.sum());
// static literal spread (parser fold)
const r = new P(...[30, 12]);
console.log(r.sum());
// extra elements ignored
const arr3: number[] = [1, 2, 3, 4];
const s = new P(...arr3);
console.log(s.sum());
// string lane
class W { s: string; constructor(s: string) { this.s = s; } }
const sa: string[] = ["hi"];
console.log(new W(...sa).s);
