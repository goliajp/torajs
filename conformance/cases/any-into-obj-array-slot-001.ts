// An `any` value bound to a `let` annotated with an array of objects
// is checked like the element slot: an array whose elements have the
// fields is copied, anything else throws a TypeError where bun reads on.
class Box {
  item: number;
  constructor(i: number) {
    this.item = i;
  }
}
function attempt(tag: string, f: () => void): void {
  try {
    f();
  } catch (e) {
    console.log(tag, e instanceof TypeError);
  }
}
const u: any = undefined;
attempt("undefined", () => {
  const t: Box[] = u;
  console.log(t.length);
});
const src: any = [{ item: 1 }, { item: 2 }];
const bs: Box[] = src;
console.log(bs.length, bs[0].item + bs[1].item);
function sum(v: any): number {
  const xs: { item: number }[] = v;
  let s = 0;
  for (const x of xs) s += x.item;
  return s;
}
console.log(sum([{ item: 3 }, { item: 4 }]));
attempt("number", () => console.log(sum(5)));
const nested: any = [[{ item: 9 }]];
const grid: Box[][] = nested;
console.log(grid[0][0].item);
