// §19.2.1 — the global `eval` as a VALUE inside closure bodies. The
// checker fallback types it Any and the runtime cell rides the any
// lanes (works at top level and in FnDecl bodies); the capture
// collector must NOT collect it as a free variable (the Iterator
// drift, free_vars_globals).
function runner(cb: any) {
  try {
    cb();
  } catch (e) {
    console.log(e instanceof TypeError);
  }
}
runner(function () {
  Promise.all.call(eval);
});

var g = function () {
  return typeof eval;
};
console.log(g());

var picked = [1, 2].map(() => eval);
console.log(typeof picked[0]);
console.log(picked[0] === eval);
