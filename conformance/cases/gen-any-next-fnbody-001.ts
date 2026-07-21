// any-typed generator object: `.next()` inside a fn body must route
// through any dispatch (speculative __cm___Gen_*__next rewrite demotes)
// exactly like the same call at top level.
function* sg() {
  yield 1;
  yield 2;
}
let it: any = sg();
function take() {
  let a = it.next();
  console.log(a.value, a.done);
}
take();
take();
let end: any = it.next();
console.log(end.value, end.done);
