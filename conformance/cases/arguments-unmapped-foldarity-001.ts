// FoldArity tier (a fn called with differing arg counts never joins
// the static-argv face): a mutating body must still get the unmapped
// view — module code is strict, `arguments[0] = 5` never reaches `a`
// and a param write never shows in a later arguments read. The
// mutation scan routes such bodies to the materialized array under
// the tier's declared-arity assumption.
function m(a: number) {
  arguments[0] = 5;
  console.log(a, arguments[0]);
}
m(1);
m(2, 9);

function w(x: number, y: number) {
  x = 77;
  console.log(arguments[0], arguments[1]);
}
w(3, 4);
w(5, 6, 7);
