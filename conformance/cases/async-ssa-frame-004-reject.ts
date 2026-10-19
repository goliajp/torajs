// A suspending async function turns every throw into the rejection of
// its promise: an awaited rejection with no try around it, a throw of
// its own after an await, and a throw raised in a function it calls,
// before and after the first await. The caller is never thrown at.

function boom(x: number): number {
  if (x > 1) {
    throw new Error("boom " + x);
  }
  return x;
}

async function awaitsRejected(): Promise<number> {
  await null;
  const v = await Promise.reject(new Error("rejected"));
  return v + 1;
}

async function throwsAfter(n: number): Promise<number> {
  await Promise.resolve(n);
  if (n > 0) {
    throw new Error("own " + n);
  }
  return n;
}

async function callsThrowing(x: number): Promise<number> {
  const a = boom(x - 1);
  await null;
  const b = boom(x);
  return a + b;
}

async function main(): Promise<void> {
  const order: string[] = [];
  const r1 = awaitsRejected().catch((e: any) => { order.push("c1:" + e.message); return -1; });
  const r2 = throwsAfter(3).catch((e: any) => { order.push("c2:" + e.message); return -2; });
  const r3 = callsThrowing(1).then((v: number) => { order.push("ok3:" + v); return v; });
  const r4 = callsThrowing(2).catch((e: any) => { order.push("c4:" + e.message); return -4; });
  const r5 = callsThrowing(3).catch((e: any) => { order.push("c5:" + e.message); return -5; });
  order.push("sync");
  const vals = await Promise.all([r1, r2, r3, r4, r5]);
  console.log(vals.join(","));
  console.log(order.join(","));
  try {
    await throwsAfter(9);
  } catch (e: any) {
    console.log("caught", e.message);
  }
}

main();
