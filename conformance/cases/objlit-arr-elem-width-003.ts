// Nested and class-held element structs widen through every level, and
// a self-referential element type still widens without looping.
type B = { v: number };
type Outer = { b: B; n: number };
const os: Outer[] = [{ b: { v: 0.5 }, n: 1 }];
console.log(os[0].b.v, os[0].n);

const grid: B[][] = [[{ v: 1 }], [{ v: 2.5 }]];
console.log(grid[0][0].v, grid[1][0].v);

class C {
  b: B;
  items: B[];
  constructor() {
    this.b = { v: 0.5 };
    this.items = [{ v: 1.5 }];
  }
}
const c = new C();
c.items.push({ v: 2.5 });
console.log(c.b.v, c.items[0].v, c.items[1].v);

class D {
  items: E[] = [];
}
type E = { v: number };
const d = new D();
d.items.push({ v: 2.5 });
console.log(d.items[0].v);

type Tree = { v: number; kids: Tree[] };
const t: Tree = { v: 0.5, kids: [{ v: 1.25, kids: [] }] };
const forest: Tree[] = [t, { v: 3, kids: [] }];
console.log(forest[0].v, forest[0].kids[0].v, forest[1].v);
