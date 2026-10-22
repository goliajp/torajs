// Promise.all over typed inputs mixing fulfilled, rejected and pending
// elements, read in one walk.
//
// A pending element anywhere sends the whole input to the fan-in, even
// when a rejected one comes before it: the reason is still that first
// rejected element's, and every input counts as handled, so the
// rejection the pending one settles with later is not reported. With
// everything settled, the first rejected element in index order wins
// and the result rejects one tick later, after the jobs queued ahead.

async function main() {
  let late: (e: number) => void = (_e: number) => {};
  const pending = new Promise<number>((_res, rej) => {
    late = rej;
  });
  const a: Promise<number>[] = [Promise.resolve(1), Promise.reject(2), pending];
  try {
    const vs = await Promise.all(a);
    console.log("A fulfilled", vs.length);
  } catch (e) {
    console.log("A rejected", e);
  }
  late(3);

  const b: Promise<number>[] = [Promise.resolve(1), Promise.reject(20), Promise.reject(30)];
  const order: string[] = [];
  const pb = (async () => {
    try {
      await Promise.all(b);
    } catch (e) {
      order.push("B rejected " + e);
    }
  })();
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
}

main();
