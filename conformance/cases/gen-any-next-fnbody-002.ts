// any receiver reached through a param + a local any rebind — both
// fn-body-only shapes of the demotion (probe-g2k family).
function* sg() {
  yield "x";
}
function pull(g: any) {
  let l: any = g;
  let r = l.next();
  console.log(r.value, r.done);
  console.log(l.next().done);
}
pull(sg());
