// RFC 20260730 knife B — Promise.all/any/race drive the real
// GetIterator protocol over a dynamic argument: any-held arrays,
// strings, Sets, custom [Symbol.iterator]() objects, and poisoned
// iterators all behave per §27.2.4 at runtime. Sequential awaits
// keep the print order microtask-schedule-independent.

async function main(): Promise<void> {
  // any-held mixed array — non-thenable elements resolve-wrap.
  const xs: any = [Promise.resolve(1), 2];
  const r1: any = await Promise.all(xs);
  console.log("all-anyarr:" + r1[0] + "," + r1[1]);

  // string — spec-iterable, one element per character.
  const r2: any = await Promise.all("ab");
  console.log("all-str:" + r2.length + ":" + r2[0] + r2[1]);

  // typed Set — default iterator is values().
  const s: Set<number> = new Set<number>();
  s.add(7);
  s.add(9);
  const r3: any = await Promise.race(s);
  console.log("race-set:" + r3);

  // custom iterator object through any.
  let n: number = 0;
  const iter: any = {};
  iter[Symbol.iterator] = function (): any {
    return {
      next: function (): any {
        n = n + 1;
        if (n <= 2) {
          return { value: n * 10, done: false };
        }
        return { value: undefined, done: true };
      },
    };
  };
  const r4: any = await Promise.all(iter);
  console.log("all-custom:" + r4[0] + "," + r4[1]);

  // poisoned next() — the thrown error becomes the rejection reason.
  const bad: any = {};
  bad[Symbol.iterator] = function (): any {
    return {
      next: function (): any {
        throw new RangeError("poisoned");
      },
    };
  };
  try {
    await Promise.any(bad);
    console.log("BAD: poisoned fulfilled");
  } catch (e) {
    const ee: any = e;
    console.log("any-poisoned:" + ee.message);
  }

  // still non-iterable: a plain object without Symbol.iterator.
  try {
    await Promise.all({ a: 1 });
    console.log("BAD: plain object fulfilled");
  } catch (e) {
    console.log("all-plain-object:" + (e instanceof TypeError));
  }
  console.log("done");
}
main();
