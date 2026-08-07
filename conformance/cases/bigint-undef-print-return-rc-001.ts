// Rotation 326 — two defects behind one census case (the BigInt rows
// of arr-index-read-generic-cell-families-001):
//
// 1. `console.log(<BigInt miss>)` read the immortal undefined cell's
//    CONTENT: the console BigInt lane (and its drifted-apart copy in
//    the coercer) called bigint_to_string on the sentinel and walked
//    limbs that aren't there — a two-line SIGBUS. Every consumer of
//    the generic-undefined-cell family branches on the ADDRESS.
const bs: bigint[] = [1n, 2n];
console.log(bs[5]);
const bempty: bigint[] = [];
console.log(bempty.pop());
console.log(bs.at(9));

// 2. returning a typed array element hands the caller a BORROW of
//    the slot (load_dyn reads in place), while the fn-return
//    contract says the caller owns every return value: the boxed
//    entry then boxed the borrow into `any` and the caller's drop
//    stole the slot's stake. The return lane now pays the +1.
const f = () => bs[0];
console.log(f());
console.log(bs[0]);
console.log("done");
