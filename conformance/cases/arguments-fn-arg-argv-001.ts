// rotation 365 fn-arg track — an anonymous argv-face fn-expr passed
// directly as a USER fn's argument (the t262 assert.throws thunk
// idiom): the receiving param is annotated, consumed by direct call
// only, and the argv tier marks that (fn, param) boxed — the SSA
// variadic registration routes the param's calls through the boxed
// dual entry, and the checker admits the rest-tail value into the
// declared closure param slot for exactly that pairing (module docs
// in ast/arguments_object_collect.rs::collect_fn_arg_argv).
function runner(thunk: () => void): void {
  thunk();
}
runner(function () {
  console.log(arguments.length);
});
runner(function () {
  var a = arguments;
  console.log(a.length);
  console.log(a[0]);
});
function taker(cb: (x: number) => number): number {
  return cb(5);
}
console.log(
  taker(function (x: number): number {
    return arguments[0] + x;
  })
);
