// §10.4.4.7 CreateUnmappedArgumentsObject step 6 — `length` is
// { [[Writable]]: true, [[Configurable]]: true }, so
// `delete arguments.length` answers true. The classifier's
// Unmapped-arm gate must SEE this body: the `.length` member node
// is dark to NonLengthTouch (absorbed) and a `delete`'s inside is
// dark to Length, so the rotation-435 Length∪NonLengthTouch gate
// classified the body off the materialized ride while the delete
// rewrite still read `__torajs_arguments` — an unknown-identifier
// reject (test262 S10.6_A5_T3's pass regression, taken back by
// the AnyTouch scan).
function f1() {
  return delete arguments.length;
}
console.log(f1());

var f2 = function () {
  return delete arguments.length;
};
console.log(f2());
