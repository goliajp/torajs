// Every member a class declares is non-enumerable (§15.7.14). The ES5
// lane used to install instance members by assignment, which makes an
// ENUMERABLE property — so `for (const k in obj)` answered with the
// method names and `Object.keys(K.prototype)` listed them. Only the
// fields the constructor writes belong in that answer.
function run(base: number) {
  class Point {
    x: number;
    y: number;
    constructor(x: number, y: number) {
      this.x = x + base;
      this.y = y;
    }
    len() {
      return this.x + this.y;
    }
    get sum() {
      return this.len();
    }
  }
  const p = new Point(1, 2);
  const own: string[] = [];
  for (const k in p as any) own.push(k);
  const proto: string[] = Object.keys(Object.getPrototypeOf(p) as any);
  return [own.join("|"), "[" + proto.join("|") + "]", p.len(), p.sum].join(",");
}

console.log(run(0));
console.log(run(10));
