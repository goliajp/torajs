// 397-02 — the alias-init closure: `const B = K` is a receiver-safe
// use of a this-reading fn-expr when every use of the alias is
// itself safe (greatest fixpoint, chains included). The motivating
// shape is the for-in head desugar, which snapshots its source into
// `const __forin_obj_N = K` — that alias's only use is the
// `Object.__forinKeys` argument, which never calls the enumerated
// object (§14.7.5.9).

const K = function () {
  return typeof this;
};
console.log(K());
for (const k in K) {
  console.log("key", k);
}

// a user-written alias chain whose uses are all value shapes
const K2 = function () {
  return typeof this;
};
console.log(K2());
const B = K2;
const C2 = B;
console.log(B === K2, C2 === K2);
for (const k in K2) {
  console.log("key2", k);
}
