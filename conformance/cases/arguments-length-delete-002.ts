// §10.4.4 — `delete arguments.length` inside the fn (the direct
// member spelling, S10.6_A5_T3): the length arm's read rewrite would
// fold the operand to a number; the delete routes as a keyed delete
// on the materialized array, where the tombstone kernel answers the
// configurable delete.
function t() {
  var had = arguments.length === 2;
  var ok = delete arguments.length;
  console.log(had, ok, arguments.hasOwnProperty("length"));
}
t(1, 2);
