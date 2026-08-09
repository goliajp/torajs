// probe aw7-forof: rewrite_returns_for_async had no ForOf arm, so a
// `return` inside an async body's for-of kept its bare value —
// annotated fns failed check (`Promise(Number)` vs `Number`),
// unannotated ones leaked the bare value out of the Promise lane.
async function firstBig(xs: number[]): Promise<number> {
  for (const x of xs) {
    if (x > 10) {
      return x;
    }
  }
  return -1;
}
async function firstOddNoAnn(xs: number[]) {
  for (const x of xs) {
    if (x % 2 === 1) {
      return x;
    }
  }
  return -1;
}
async function main() {
  const p = firstBig([3, 22, 5]);
  console.log(typeof p);
  console.log(await p);
  console.log(await firstBig([1, 2]));
  const q = firstOddNoAnn([4, 9, 6]);
  console.log(typeof q);
  console.log(await q);
}
main();
