// The array `map` builds must take its element width from the analysis
// class that owns the product, not from the callback's return type.
//
// The callback's ret is only ONE source of the product's elements. The
// analysis keys the product by its call origin and joins that class with
// every slot the product flows into, so a fractional value reaching any
// of those slots widens the class while the callback's ret edge stays
// narrow — that edge is directional on purpose (reduce's accumulator
// rides it). Minting the array from the ret alone stored i64 bits behind
// an f64-typed slot, and every later read reinterpreted them: `4` came
// back as `2e-323`, silently, with the process still exiting 0.

const xs: number[] = [1, 2, 3];
const doubled: number[] = xs.map((x: number): number => x * 2);
doubled[0] = 1.5;
console.log(doubled[0]);
console.log(doubled[1]);
console.log(doubled[2]);

// the same split reached through a later element write rather than the
// first slot — the class is one, so the position must not matter
const ys: number[] = [10, 20];
const tripled: number[] = ys.map((x: number): number => x * 3);
tripled[1] = 0.5;
console.log(tripled[0], tripled[1]);

// an all-integral product stays narrow, and reads back as integers
const zs: number[] = [4, 5];
const plus: number[] = zs.map((x: number): number => x + 1);
console.log(plus[0], plus[1]);

// a callback that is itself fractional already widened before this fix —
// the regression guard for it
const ws: number[] = [1, 2];
const halved: number[] = ws.map((x: number): number => x / 2);
console.log(halved[0], halved[1]);

// chained: the product of one map feeds another
const chained: number[] = xs.map((x: number): number => x * 2).map((x: number): number => x + 1);
chained[0] = 2.5;
console.log(chained[0], chained[1], chained[2]);
