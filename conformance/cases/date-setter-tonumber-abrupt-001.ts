// rotation 365 — `? ToNumber(arg)` abrupt completion in Date setters
// (§21.4.4.20 step 2 et al.): a user valueOf throw recorded by the
// any-lane ToNumber decode must abort BEFORE the kernel writes
// [[DateValue]] — previously the pending throw was silently dropped
// and the date absorbed the garbage coercion (double silent-wrong).
// decode_fields answers None on a pending record and every setter
// arm short-circuits; the typed lane's coerce_date_num grew the
// mirror check. Covers 1-arg and multi-arg setters plus the
// unchanged-date assertion both times, and a plain setTime tail
// guards the happy path.
var date = new Date(0);
var originalValue = date.getTime();
var obj: any = {
  valueOf: function () {
    throw new Error("boom");
  },
};
var threw = false;
try {
  date.setDate(obj);
} catch (e) {
  threw = true;
}
console.log(threw);
console.log(date.getTime() === originalValue);
var threw2 = false;
try {
  date.setFullYear(2020, obj);
} catch (e) {
  threw2 = true;
}
console.log(threw2);
console.log(date.getTime() === originalValue);
console.log(new Date(5).setTime(77));
