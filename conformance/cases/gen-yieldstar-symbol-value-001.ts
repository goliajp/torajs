// async-gen `yield*` over a hand-rolled async iterator whose step
// value is a Symbol — rotation 288: `__torajs_anyv_await`'s identity
// arm returned a non-promise cell without the +1 stake the lowering
// contract expects, so the owned step object (and its Symbol slot)
// was freed while still bound (`__gfstep` dangled; release-only
// layout-sensitive stdout mismatch, UAF under Guard Malloc).
var obj: any = {
  [Symbol.asyncIterator]() {
    return {
      next() {
        return { value: Symbol('oi'), done: false };
      }
    };
  }
};
async function* gen() {
  yield* obj;
}
var it: any = gen();
it.next().then((r: any) => {
  console.log(typeof r.value);
  console.log(r.value.description);
  console.log(r.done);
});
