// §27.1.3.1 %AsyncIteratorPrototype%[@@asyncIterator] — an async generator object
// answers a function for `[Symbol.asyncIterator]` that returns the receiver, so a
// GetMethod-driven consumer (Array.fromAsync, a hand-written for-await loop) finds
// the async protocol on it. The sync kind keeps inheriting @@iterator.
async function* ag(): any {
  yield 1;
  yield 2;
}
function* sg(): any {
  yield 3;
}
async function main(): Promise<void> {
  const g: any = ag();
  const s: any = sg();
  console.log(typeof g[Symbol.asyncIterator], typeof g[Symbol.iterator], typeof s[Symbol.iterator], typeof s[Symbol.asyncIterator]);
  console.log(g[Symbol.asyncIterator]() === g, s[Symbol.iterator]() === s);
  const it: any = g[Symbol.asyncIterator].call(g);
  const out: number[] = [];
  while (true) {
    const r: any = await it.next();
    if (r.done) {
      break;
    }
    out.push(r.value);
  }
  console.log(out.join(","));
  console.log((await Array.fromAsync(ag())).join(","));
  console.log("done");
}
main();
