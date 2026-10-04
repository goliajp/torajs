// An array reaching a typed array parameter through `any` is read as
// the array it is: a block of boxed values, a block of another element
// kind, a nested array of the wrong kind or a sparse array all give
// the body their own elements back, as do rest parameters.
class Reader {
  n: number = 0;
  firstNum(xs: number[]): number {
    return xs[0] + 1;
  }
  firstStr(xs: string[]): string {
    return xs[0] + "!";
  }
  firstLen(xs: string[]): number {
    return xs[0].length;
  }
  kindOf(xs: string[]): string {
    return typeof xs[0];
  }
  inner(xs: number[][]): number {
    return xs[0][0] + 1;
  }
  flags(xs: boolean[]): string {
    return typeof xs[0];
  }
  sum(...xs: number[]): number {
    let t = 0;
    for (const x of xs) t += x;
    return t;
  }
  join(...xs: string[]): string {
    return xs.join("-");
  }
}

const r: any = new Reader();
const nums: number[] = [1.5, 2.5];
const strs: string[] = ["ab", "cd"];
const grid: number[][] = [[4, 5]];
const parsed: any = JSON.parse("[1, 2]");
const mixed: any = [7, "x"];
const holes: any = [];
holes[3] = 9;

console.log(r.firstNum(nums), r.firstStr(strs), r.inner(grid), r.flags([true]));
console.log(r.firstNum(parsed), r.firstNum(mixed), r.firstNum(holes));
console.log(r.firstStr(nums), r.firstStr(parsed), r.firstStr(mixed), r.firstStr(holes));
console.log(r.inner([["3"]]), r.inner([parsed]), r.inner(parsed), r.flags(nums));
console.log(r.kindOf(nums), r.kindOf(parsed), r.kindOf(grid), r.kindOf(strs));
for (const bad of [[null], [undefined], strs]) {
  try {
    console.log("firstLen", r.firstLen(bad));
  } catch (e) {
    console.log("firstLen", e instanceof TypeError);
  }
}
console.log(r.sum(1, 2, 3), r.sum(), r.sum(1.5, 2), r.join("a", "b"), r.join());
