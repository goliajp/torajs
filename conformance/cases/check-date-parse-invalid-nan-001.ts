// `Date.parse(s)` per ES §21.4.3.2: spec returns NaN when `s` is
// not a valid date string. tora's `__torajs_date_parse_iso`
// signaled failure via the `INT64_MIN` sentinel, declared the
// intrinsic with an i64 return type, and propagated the sentinel
// unchanged — `Number.isNaN(Date.parse('invalid'))` returned
// false, `console.log(Date.parse('invalid'))` printed
// `-9223372036854775808`. Fix changes the helper return to f64
// (parse failure → `f64::NAN`, success → `ms as f64`) and the
// intrinsic declare to `Type::F64`. f64 ms values fit exactly
// for timestamps until ~year 285616 (< 2^53 ms beyond epoch),
// so valid Date strings still round-trip integer-form via
// `f64_to_str`.

console.log(Date.parse('2026-06-19'));
console.log(Date.parse('2026-06-19T10:30:00Z'));
console.log(Date.parse('1970-01-01T00:00:00Z'));

console.log(Date.parse('invalid'));
console.log(Date.parse('not-a-date'));
console.log(Date.parse(''));

console.log(Number.isNaN(Date.parse('invalid')));
console.log(Number.isNaN(Date.parse('2026-06-19')));
console.log(typeof Date.parse('invalid'));
console.log(typeof Date.parse('2026-06-19'));
