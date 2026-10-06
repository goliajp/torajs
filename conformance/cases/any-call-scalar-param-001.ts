// a number / boolean parameter reached through `any` binds the argument as passed
function inc(n: number): any { return n + 1; }
const f: any = inc;
console.log(f("7"), f(true), f(null), f(undefined), f(2), inc(41));

function same(b: boolean): any { return b === true ? "T" : typeof b; }
const g: any = same;
console.log(g(1), g("x"), g(true), same(false));

function sum(a: number, b: number): number { return a + b; }
const h: any = sum;
console.log(h(1, 2), h("a", 2), sum(3, 4));

class Box {
  k: number;
  constructor(k: number) { this.k = k; }
  add(n: number): any { return n + this.k; }
  flag(b: boolean): any { return typeof b; }
}
const bx: any = new Box(1);
const bt = new Box(2);
console.log(bx.add("4"), bx.add(4), bx.flag(0), bx.flag(false), bt.add(5));

const twice = (n: number): any => n + n;
const tw: any = twice;
console.log(tw("ab"), tw(3), tw([1]), twice(4));

function dflt(n: number = 10, b: boolean = true): any { return [n + 1, typeof b, b]; }
const df: any = dflt;
console.log(df("3"), df(undefined, 0), df(), df(2, false), dflt(5));

const ys: number[] = [3, 1, 2];
ys.sort((a: number, b: number) => a - b);
const zs: number[] = [5, 4];
zs[3] = 1;
zs.sort((a: number, b: number) => a - b);
console.log(ys, zs.length, zs[0], zs[1], zs[2], 3 in zs);
