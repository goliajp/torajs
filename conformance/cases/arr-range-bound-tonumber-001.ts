// ES §23.1.3.4 / §23.1.3.7 — copyWithin / fill read every bound
// through ToIntegerOrInfinity, which refuses no value. The lane used
// to admit Number | Undefined | Any by name and refuse the rest.
const boolStart = [1, 2, 3, 4, 5];
console.log(boolStart.copyWithin(0, true).join(","));

const nullStart = [1, 2, 3, 4, 5];
console.log(nullStart.copyWithin(0, null).join(","));

const vo = { valueOf() { return 1; } };
const objStart = [1, 2, 3, 4, 5];
console.log(objStart.copyWithin(0, vo).join(","));

const strEnd = [1, 2, 3, 4, 5];
console.log(strEnd.copyWithin(0, 1, "3").join(","));

const filled = [1, 2, 3, 4, 5];
console.log(filled.fill(9, true).join(","));

const vo2 = { valueOf() { return 2; } };
const filled2 = [1, 2, 3, 4, 5];
console.log(filled2.fill(9, vo2).join(","));

// >=4-arg trailing-ignore shape is the same lane: the bound before
// the ignored trailing arg must widen too.
const trailing = [1, 2, 3, 4, 5];
console.log(trailing.copyWithin(0, true, 4, 99).join(","));

// An `any` may be HOLDING undefined; §23.1.3.4 step 8 reads that as
// the slot's default (len), not as ToNumber's NaN -> 0.
const u: any = undefined;
const anyUndefEnd = [1, 2, 3, 4, 5];
console.log(anyUndefEnd.copyWithin(0, 1, u).join(","));

const anyUndefStart = [1, 2, 3, 4, 5];
console.log(anyUndefStart.fill(8, u).join(","));

// A user valueOf runs at the bound slot and may throw.
const boom = { valueOf(): number { throw new Error("boom"); } };
const throwing = [1, 2, 3, 4, 5];
try {
  throwing.copyWithin(0, boom);
} catch (e) {
  console.log("caught " + (e as Error).message);
}
console.log(throwing.join(","));
