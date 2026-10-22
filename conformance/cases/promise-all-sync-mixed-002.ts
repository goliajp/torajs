// promise-all-sync-mixed-001's shapes in a program that writes no
// `then` and no Promise static, so the kernel is told it may skip the
// own-`then` probe and the patch consult. The lane each input takes,
// the reason it rejects with and the tick it settles on stay the same.

async function main() {
  let late: (e: number) => void = (_e: number) => {};
  const pending = new Promise<number>((_res, rej) => {
    late = rej;
  });
  const a: Promise<number>[] = [Promise.resolve(1), Promise.reject(2), pending];
  const ra = await Promise.all(a).then(
    (vs) => "A fulfilled " + vs.length,
    (e: any) => "A rejected " + e,
  );
  console.log(ra);
  late(3);

  const b: Promise<number>[] = [Promise.resolve(1), Promise.reject(20), Promise.reject(30)];
  const order: string[] = [];
  const pb = Promise.all(b).then(
    (vs) => order.push("B fulfilled " + vs.length),
    (e: any) => order.push("B rejected " + e),
  );
  Promise.resolve(0).then(() => order.push("tick 1"));
  Promise.resolve(0)
    .then(() => order.push("tick 1b"))
    .then(() => order.push("tick 2"));
  await pb;
  await Promise.resolve(0);
  console.log(order.join(", "));

  const c: Promise<number>[] = [Promise.resolve(4), Promise.resolve(5), Promise.resolve(6)];
  const rc = await Promise.all(c);
  console.log("C", rc[0] + rc[1] + rc[2], rc.length);

  const empty: Promise<number>[] = [];
  console.log("empty", (await Promise.all(empty)).length);
}

main();
