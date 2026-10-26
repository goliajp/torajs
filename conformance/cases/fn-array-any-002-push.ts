// `push` / `unshift` / `splice` / `fill` take an `any` function into a
// typed function array, and a call through the element answers what the
// stored function returns.
const half: any = (x: any) => x * 0.5;
const tag: any = (x: any) => "v" + x;
const both: any = (a: any, b: any) => (b === undefined ? "u" : "d") + a;

const hs: ((x: number) => number)[] = [];
hs.push(half);
hs.unshift(tag);
console.log(hs.length, hs[0](2), hs[1](2));
hs.push(both, (x: number) => x - 1);
console.log(hs.length, hs[2](7), hs[3](7));
hs.splice(1, 0, both);
console.log(hs.length, hs[1](8));
hs.fill(half, 3);
console.log(hs[3](3), hs[4](5));

class Box {
  fs: ((x: number) => number)[] = [];
  run(v: number): number {
    return this.fs[0](v);
  }
}
const b = new Box();
b.fs.push(half);
console.log(b.run(3));
b.fs[0] = tag;
console.log(b.run(3));

const fn2: any = function (this: any, x: any) {
  return Array.isArray(this) ? "this-is-array " + x : "no";
};
hs[0] = fn2;
console.log(hs[0](9));
