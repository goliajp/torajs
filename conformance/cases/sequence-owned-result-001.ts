// A comma answers what its last operand lowered to: a call there hands
// its consumer the reference it minted, in statement, argument, await
// and initializer positions alike, and a borrow there stays a borrow.

function mk(i: number): number[] {
  return [i, i + 1];
}
function take(xs: number[]): number {
  return xs[1];
}
function label(i: number): string {
  return "v" + i;
}
async function later(i: number): Promise<number[]> {
  return [i];
}

const keep = [7, 8];
let k = 0;
let s = 0;
for (let i = 0; i < 1000; i++) {
  (k++, mk(i));
  s += take((0, mk(i)));
  s += take((k, keep));
  const t = (0, label(i));
  s += t.length;
}
console.log(k, s, keep.join());

const held = (0, keep);
held.push(9);
console.log(keep.join(), held === keep);

class Base {
  constructor(o: any) {
    if (o) return o;
  }
}
class D extends Base {
  x = 1;
  #y = 2;
  constructor(o: any) {
    super(o);
  }
  y() {
    return this.#y;
  }
}
let d = 0;
for (let i = 0; i < 1000; i++) {
  const v: any = new D(i % 2 ? { z: 3 } : null);
  d += v.x + (i % 2 ? v.z : v.y());
}
console.log(d);

async function main() {
  let a = 0;
  for (let i = 0; i < 1000; i++) {
    const xs = await (0, later(i));
    a += xs[0];
  }
  console.log(a);
}
main();
