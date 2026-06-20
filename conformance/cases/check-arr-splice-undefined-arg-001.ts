// S237 — `Array<T>.splice(start, undefined)` /
// `Array<T>.toSpliced(start, undefined)` per ES §23.1.3.31 step 7:
// ToIntegerOrInfinity(undefined) = 0, so an explicit-undefined
// deleteCount yields 0 removal (distinct from the omitted-arg
// 1-arg shape which spec-defaults to `len - actualStart`). The
// check.rs S237 trims `params` and `effective_args` to the 1-arg
// shape when arg 1 is Undefined; ssa_lower_splice / ssa_lower_tospliced
// detect the same shape and substitute ConstI64(0) for deleteCount
// instead of the 1-arg i64::MAX sentinel.
const xs: number[] = [1, 2, 3, 4, 5];
console.log(xs.splice(1, undefined));
console.log(xs);
const ys: number[] = [10, 20, 30];
console.log(ys.toSpliced(1, undefined));
console.log(ys);
const zs: number[] = [];
console.log(zs.splice(0, undefined));
console.log(zs);
