// rotation 362 — ternary (typed : null) joins: a register-repr or
// struct branch against `null` joins to Any (both sides box; the
// null side folds to ANY_NULL). Ptr-repr branches keep Nullable.
const arr = [1.5, 2.5];
const i = 0;
console.log(i < 5 ? arr[i] : null); // f64 lane, then side
console.log(i > 5 ? arr[i] : null); // f64 lane, null side
console.log(i > 5 ? 7 : null); // i64 lane, null side (was silent 0)
console.log(i < 5 ? 7 : null); // i64 lane, then side
console.log(i < 5 ? true : null); // bool lane
console.log(i > 5 ? true : null); // bool lane, null side
const o = i > 5 ? { a: 1 } : null; // struct lane, null side
console.log(o);
const o2 = i < 5 ? { a: 1 } : null; // struct lane, live side
console.log(o2);
console.log(i < 5 ? "hi" : null); // ptr-repr keeps Nullable<Str>
console.log(i > 5 ? "hi" : null);
console.log(i > 5 ? [1, 2] : null); // ptr-repr keeps Nullable<Arr>
const flip = i < 5 ? null : 9; // null in THEN position
console.log(flip);
