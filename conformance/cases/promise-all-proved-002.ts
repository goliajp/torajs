// Promise.all under both whole-program proofs over a typed input: the
// elements are promise cells, settled or not. Settled inputs answer in
// one walk, a pending one is waited on, and the first rejected element
// in index order rejects the result, one tick after the jobs queued
// ahead of it.
let fire: (v: number) => void = (_v: number) => {};
const pending = new Promise<number>((res) => {
  fire = res;
});
let fail: (e: string) => void = (_e: string) => {};
const failing = new Promise<number>((_res, rej) => {
  fail = rej;
});
async function main() {
  const order: string[] = [];
  const settled: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2)];
  const waiting: Promise<number>[] = [Promise.resolve(3), pending, Promise.resolve(5)];
  const r1 = await Promise.all(settled);
  const p2 = Promise.all(waiting);
  fire(4);
  const r2 = await p2;
  console.log(r1[0] + r1[1], r2[0] + r2[1] + r2[2], r2.length);
  const rejecting: Promise<number>[] = [failing, Promise.reject("first"), Promise.reject("second")];
  const p3 = Promise.all(rejecting).then(
    (v) => order.push("fulfilled " + v.length),
    (e) => order.push("rejected " + e),
  );
  Promise.resolve(0).then(() => order.push("tick"));
  fail("late");
  await p3;
  console.log(order.join(", "));
}
main();
