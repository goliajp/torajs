// proposal-array-from-async §2.1.1 — the mapfn form. Per element
// the spec interleaves await / mapfn(value, k) / await-the-mapped-
// result, so mapfn HAS run for elements before a later throw and
// has NOT run past it; step 2's IsCallable check precedes iteration
// (a non-callable mapfn rejects even for an empty source).

async function main(): Promise<void> {
  // mapfn(value, k) over a mixed source — elements unwrap first.
  const xs: any = [Promise.resolve(1), 2, Promise.resolve(3)];
  const r1 = await Array.fromAsync(xs, function (v: any, k: any): any {
    return v * 10 + k;
  });
  console.log(r1[0], r1[1], r1[2]);

  // mapfn over a string source.
  const r2 = await Array.fromAsync("ab", function (c: any): any {
    return c + "!";
  });
  console.log(r2[0], r2[1]);

  // mapfn returning a promise — the mapped value awaits too.
  const ys: any = [1, 2];
  const r3 = await Array.fromAsync(ys, function (v: any): any {
    return Promise.resolve(v + 5);
  });
  console.log(r3[0], r3[1]);

  // mapfn throws — the result rejects; earlier elements' effects ran.
  const zs: any = [1, 2];
  try {
    await Array.fromAsync(zs, function (v: any): any {
      if (v === 2) {
        throw new RangeError("boom");
      }
      console.log("mapped", v);
      return v;
    });
    console.log("BAD: fulfilled");
  } catch (e) {
    const ee: any = e;
    console.log("threw:", ee.message);
  }

  // non-callable mapfn rejects TypeError even on an empty source.
  const empty: any = [];
  const one: any = 1;
  try {
    await Array.fromAsync(empty, one);
    console.log("BAD: noncallable fulfilled");
  } catch (e) {
    console.log("noncallable:", e instanceof TypeError);
  }
  console.log("done");
}
main();
