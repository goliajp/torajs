// RFC 20260730 knife A — ES §27.2.4.{1,2,3,5}: a Promise combinator
// handed a non-iterable argument answers a promise REJECTED with a
// TypeError at runtime; it is not a compile-time reject and not a
// synchronous throw. Statically non-iterable primitives route
// through the __torajs_promise_*_dyn kernel entries.

const pAll: any = Promise.all(3);
pAll.then(
  function (): void {
    console.log("BAD: all fulfilled");
  },
  function (e: any): void {
    console.log("all rejected TypeError=" + (e instanceof TypeError));
  }
);

const pRace: any = Promise.race(true);
pRace.then(
  function (): void {
    console.log("BAD: race fulfilled");
  },
  function (e: any): void {
    console.log("race rejected TypeError=" + (e instanceof TypeError));
  }
);

const pAny: any = Promise.any(null);
pAny.then(
  function (): void {
    console.log("BAD: any fulfilled");
  },
  function (e: any): void {
    console.log("any rejected TypeError=" + (e instanceof TypeError));
  }
);

const pSettled: any = Promise.allSettled(undefined);
pSettled.then(
  function (): void {
    console.log("BAD: allSettled fulfilled");
  },
  function (e: any): void {
    console.log("allSettled rejected TypeError=" + (e instanceof TypeError));
  }
);

console.log("sync-after");
