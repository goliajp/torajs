// Promise.all over a typed array whose middle element wears its own
// `then` (§27.2.4.1.3 step 6.r invokes it per element), next to an
// element that carries some other own property only.
//
// The program writes `then`, so no whole-program proof lets the kernel
// skip the per-element probe; an expando bag without `then` must not
// send the input down the override lane either.

async function main() {
  const arr = [Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)];
  let calls = 0;
  (arr[1] as any).then = function (res: (v: number) => void, _rej: (e: unknown) => void) {
    calls++;
    res(20);
  };
  await Promise.all(arr).then((r: any) => console.log("A", r[0], r[1], r[2], calls));

  const brr = [Promise.resolve(4), Promise.resolve(5)];
  (brr[0] as any).tag = "x";
  const s = await Promise.all(brr);
  console.log("B", s[0], s[1], (brr[0] as any).tag);
}

main();
