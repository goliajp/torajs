// A typed array parameter reached through `any` reads only a block of
// its own element kind: a block of boxed values, an `f64` block behind
// a `string[]` parameter, a sparse array or a nested block of the wrong
// kind is bound as passed instead of being read as raw slots.
class R {
  n: number = 0;
  firstNum(xs: number[]): number {
    return xs[0] + 1;
  }
  firstStr(xs: string[]): string {
    return xs[0] + "!";
  }
  inner(xs: number[][]): number {
    return xs[0][0] + 1;
  }
  rows(xs: any[][]): number {
    return xs[0].length;
  }
}
const t = new R();
console.log(t.firstNum([1.5]), t.firstStr(["a"]), t.inner([[2]]));
const r: any = t;
console.log(r.firstNum(JSON.parse("[1, 2]")));
const mixed: any = [7, "x"];
console.log(r.firstNum(mixed));
const fs: number[] = [1.5, 2.5];
console.log(r.firstStr(fs));
const holes: any = [];
holes[3] = 9;
console.log(r.firstNum(holes));
console.log(r.inner([JSON.parse("[5]")]));
const strs: string[] = ["abc", "d"];
const maps: Map<string, number>[] = [new Map()];
const dates: Date[] = [new Date(0)];
const grid: number[][] = [[1, 2, 3]];
console.log(r.rows(grid), r.rows(strs), r.rows([[]]));
console.log(r.firstStr(maps), r.inner(dates), r.inner(maps), r.inner(strs));
