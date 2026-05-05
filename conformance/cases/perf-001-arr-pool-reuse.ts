// P1 Array LIFO pool — verify recycled blocks are observably
// reset (header zeroed, len=0, cap preserved) so a tight loop
// that allocates / drops same-cap arrays produces the same
// content each iter. If the pool ever returned a block with
// stale len or stale slot data leaking through, the printed
// values would diverge from the oracle.

function makeAndSum(seed: number): number {
  // 16-elem cap literal — exactly the rpn-eval shape that should
  // hit the cap-indexed pool from the second iter onward.
  let xs: number[] = [
    seed + 0, seed + 1, seed + 2,  seed + 3,
    seed + 4, seed + 5, seed + 6,  seed + 7,
    seed + 8, seed + 9, seed + 10, seed + 11,
    seed + 12, seed + 13, seed + 14, seed + 15,
  ];
  let s: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    s = s + xs[i];
  }
  return s;
}

let total: number = 0;
for (let i: number = 0; i < 1000; i = i + 1) {
  total = total + makeAndSum(i);
}
console.log(total);

// Mixed cap allocations from the same loop — prove pool keys on
// cap exactly (cap=4 and cap=16 must not share a slot, otherwise
// the load_dyn would read garbage past the small array's bounds).
function smallSum(): number {
  let ys: number[] = [10, 20, 30, 40];
  return ys[0] + ys[1] + ys[2] + ys[3];
}

let total2: number = 0;
for (let i: number = 0; i < 1000; i = i + 1) {
  total2 = total2 + makeAndSum(i) + smallSum();
}
console.log(total2);

// Large cap (above POOL_CAP_MAX = 32) bypasses the pool. Verify
// it still allocates / frees correctly without entering the
// recycling path.
function bigSum(): number {
  let zs: number[] = [];
  for (let i: number = 0; i < 50; i = i + 1) {
    zs.push(i);
  }
  let s: number = 0;
  for (let i: number = 0; i < zs.length; i = i + 1) {
    s = s + zs[i];
  }
  return s;
}

let total3: number = 0;
for (let i: number = 0; i < 100; i = i + 1) {
  total3 = total3 + bigSum();
}
console.log(total3);
