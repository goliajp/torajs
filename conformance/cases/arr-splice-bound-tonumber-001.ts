// ES §23.1.3.31 steps 3/7 and §23.1.3.34 read splice / toSpliced's
// start and deleteCount through ToIntegerOrInfinity, which refuses no
// value. The lowering side already took that road -- it reads both
// slots through `lower_to_index_operand` and tells an explicit
// `undefined` deleteCount (spec: 0) from an omitted one (spec:
// `len - actualStart`) by asking the static type, not by counting
// arguments. Only the checker still refused, and only on arg 0: the
// arity-narrow wedge had widened arg 1 and nothing had widened arg 0,
// so `x.splice(1, undefined)` ran while `x.splice(undefined, undefined)`
// did not.
const vo = { valueOf() { return 1; } };

// the two shapes that already worked -- they must not move
console.log(JSON.stringify([1, 2, 3, 4].splice(1)));
console.log(JSON.stringify([1, 2, 3, 4].splice(1, undefined)));

// explicit undefined is ToIntegerOrInfinity(undefined) = 0, an omitted
// deleteCount is `len - actualStart`. Same spelling count, different
// answers, and that difference is the whole point of these two lines.
console.log(JSON.stringify([1, 2, 3, 4].splice(undefined)));
console.log(JSON.stringify([1, 2, 3, 4].splice(undefined, undefined)));
console.log(JSON.stringify([1, 2, 3, 4].toSpliced(undefined)));
console.log(JSON.stringify([1, 2, 3, 4].toSpliced(undefined, undefined)));

console.log(JSON.stringify([1, 2, 3, 4].splice(vo)));
console.log(JSON.stringify([1, 2, 3, 4].splice(1, vo)));
console.log(JSON.stringify([1, 2, 3, 4].toSpliced(vo)));
console.log(JSON.stringify([1, 2, 3, 4].toSpliced(1, vo)));
console.log(JSON.stringify([1, 2, 3, 4].splice(null)));
console.log(JSON.stringify([1, 2, 3, 4].splice(1, null)));

// ToIntegerOrInfinity goes through ToNumber, so valueOf wins over
// toString -- this is what the two test262 A2.x cases pin down.
const both = { valueOf() { return 0; }, toString() { return 3; } };
console.log(JSON.stringify([0, 1, 2, 3].splice(both, 3)));

// a user valueOf that throws must reach the user, not a compile-time
// refusal, and it must run before anything is removed.
const boom = { valueOf() { throw new Error("boom"); } };
const keep = [1, 2, 3, 4];
try { keep.splice(boom); console.log("no-throw"); } catch (err) { console.log("start", (err as Error).message); }
console.log(JSON.stringify(keep));
try { keep.splice(0, boom); console.log("no-throw"); } catch (err) { console.log("count", (err as Error).message); }
console.log(JSON.stringify(keep));

// items from arg 2 on stay element-typed -- a bound and an element are
// different questions, and this line proves the items arm still owns them.
console.log(JSON.stringify([1, 2, 3, 4].splice(1, 2, 9, 9)));
