// Reassigned non-Copy params copy in an owned stake (materialize_fn_params).
// The default-param guard `if (x === undefined) x = d` clears the param's
// borrow booking at compile time while only firing on a runtime branch: on
// the explicit-argument path the fn-exit drop then released the CALLER's
// stake. With a generator default the freed cell's address was recycled by
// the very next __Gen alloc, whose ctor-side IteratorClose (empty-pattern
// destructure) then closed ITSELF — next() answered {done:true} without
// ever running the body (t262 gen dstr/dflt-ary-ptrn-empty family).
let c4 = 0,
  c5 = 0;
function* f4([] = [9]) {
  c4 = c4 + 1;
}
f4().next();
const it: any = (function* () {})();
function* f5([] = it) {
  c5 = c5 + 1;
}
f5().next();
console.log(c4, c5);
// chained receiver + explicit argument + consumed IteratorResult
let side = 0;
const it2: any = (function* () {
  side = 99;
  yield 7;
})();
function* f6([] = it2) {
  c5 = c5 + 100;
  yield 42;
}
const r: any = f6(it2).next();
console.log(side, r.done, r.value, c5);
