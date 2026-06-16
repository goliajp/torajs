// S133-3 — `arr.length = N` routes through the spec
// §9.4.2.4 length setter (ToUint32(v) == ToNumber(v) — RangeError
// otherwise) instead of being silently stashed in the array's
// props_dynobj side-table as a `"length"` string key.
//
// Before S133, `arr.length = N` matched the T-29 generic
// `arr.x = v` arm (props_dynobj write) — invalid values silently
// succeeded and the array's real length never validated. The fix
// reuses the `arr_set_length_validate` helper that
// Object.defineProperty(arr, "length", ...) already calls (T-29.b).
//
// Note: the runtime still can't shrink Array storage on a valid
// length, so `arr.length = N` with N < arr.length is a silent
// no-op (recorded in plan-state L3b). This fixture exercises only
// the RangeError path — that's the bit the narrow fix actually
// changes vs. the legacy props_dynobj behaviour.

// 1. Negative length — RangeError per spec §9.4.2.4.
let a = [1, 2, 3];
try {
  a.length = -1;
  console.log("no-throw");
} catch (e: any) {
  console.log("neg", e.message);
}

// 2. Fractional length — RangeError per spec.
let b = [10, 20];
try {
  b.length = 3.5;
  console.log("frac no-throw");
} catch (e: any) {
  console.log("frac", e.message);
}

// 3. NaN length — RangeError per spec (ToUint32(NaN) == 0, but
//    ToNumber(NaN) == NaN, so they're not equal).
let c = [1, 2];
try {
  c.length = NaN;
  console.log("nan no-throw");
} catch (e: any) {
  console.log("nan", e.message);
}

// 4. Valid integer length within range — succeeds (runtime no-op
//    for now; the validate path returns OK and the array's actual
//    length doesn't change since we can't shrink storage yet).
let d = [1, 2, 3, 4, 5];
d.length = 3;
console.log("valid-ok");
