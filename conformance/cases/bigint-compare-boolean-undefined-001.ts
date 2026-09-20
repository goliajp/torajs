// §7.2.13 IsLessThan reaches step 4 with ToNumeric of BOTH sides, so a
// Boolean (1/0) and an undefined (NaN) compare against a BigInt exactly
// as the Number they coerce to. The checker admitted these pairs; the
// lowering still carried a hand-written list of the other side's types
// and answered off a lane that had no rule for them — `1n > true` said
// true where the spec says false, which is worse than the compile error
// it replaced. This fixture is the one that would have caught it.
const one: bigint = 1n;
const zero: bigint = 0n;
const neg: bigint = -3n;
const t: boolean = true;
const f: boolean = false;

console.log(one > t, one < t, one >= t, one <= t);
console.log(zero > f, zero < f, zero >= f, zero <= f);
console.log(neg > t, t > neg, neg > f, f > neg);
console.log(one > f, f > one);

// ToNumeric(undefined) is NaN, and every relational comparison against
// NaN is false — including `>=`, which is NOT the negation of `<` here.
const u: undefined = undefined;
console.log(one > u, one < u, one >= u, one <= u);

// null coerces to 0 and answers normally.
const n: null = null;
console.log(one > n, zero > n, neg > n, n > neg);

// The pairs that already worked, kept beside the new ones so a future
// widening of the lowering has to keep them working too.
const num: number = 0;
const str: string = "0";
console.log(one > num, one > str, one > 2n);
