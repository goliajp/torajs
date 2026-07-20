// RFC 20260720-splice-insert knife 2 — the ES §23.1.3.31 `...items`
// insert form on typed arrays (was arity-rejected: "expected 2
// argument(s), got 3"). Matrix: replace / pure-insert past cap /
// tail-append / delete-more-than-insert / negative start / any item
// into number[] (checker admit + coerce_push_value unbox) / string
// elems (rc lane). The removed return value and receiver contents
// both print — bun parity is the acceptance.
const out: number[] = [1, 2, 3];
const removed = out.splice(1, 1, 9);
console.log(out, removed);

const grow: number[] = [1, 2];
grow.splice(1, 0, 7, 8, 9);
console.log(grow);

const tail: number[] = [1, 2];
tail.splice(2, 0, 5);
console.log(tail);

const shrink: number[] = [1, 2, 3, 4, 5];
const cut = shrink.splice(1, 3, 9);
console.log(shrink, cut);

const neg: number[] = [1, 2, 3];
neg.splice(-1, 1, 9);
console.log(neg);

const a: any = 42;
const nums: number[] = [1, 2, 3];
nums.splice(1, 1, a);
console.log(nums);

const strs: string[] = ["a", "b", "c"];
const gone = strs.splice(1, 1, "x", "y");
console.log(strs, gone);
