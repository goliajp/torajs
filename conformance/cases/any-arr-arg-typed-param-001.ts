// RFC 20260802-any-arg-typed-param-mono — an `any[]` argument (empty
// literal + push) flows into a typed `number[]` param per TS any-
// assignability. The callee is re-instantiated with the param widened
// (monomorph clone + call retarget), so its body reads the NaN-boxed
// block through the kind-aware any lanes. Mutation must flow through
// (reference semantics — the clone shares the block, never a copy).
function chk(arr: number[], message: string): boolean {
  arr.forEach(function (e, i) {
    if (e !== (i + 1)) { throw new Error(message + " '" + arr.join(",") + "'"); }
  });
  return true;
}
function grow(xs: number[]) {
  xs.push(9);
  return xs.length;
}
var seq = [];
seq.push(1);
seq.push(2);
console.log(chk(seq, "bad"));
console.log(grow(seq));
console.log(seq.length);
console.log(seq[2]);
