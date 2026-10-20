// an array of the class held in its own field can carry the undefined
// sentinel read past the end of another array: the release must keep
// testing every cell it takes from the array
class Expr {
  kind: number;
  kids: Expr[];
  constructor(kind: number, kids: Expr[]) {
    this.kind = kind;
    this.kids = kids;
  }
}
const pool: Expr[] = [new Expr(0, []), new Expr(0, [])];
let total = 0;
for (let i = 0; i < 2000; i++) {
  const t = new Expr(1, [pool[0], pool[i % 4]]);
  total += t.kids.length;
}
console.log(total);
