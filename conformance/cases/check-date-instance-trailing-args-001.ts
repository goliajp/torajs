// ES §21.4.4.* — Date instance 0-arg getter / format method
// trailing-arg ignore. Spec methods either take 0 args or have
// optional `key`/`locales`/`options` that tora's subset silently
// drops; SSA-emit default branch only forwards `recv_op`, so
// trailing user args naturally drop. S261 widens check.rs's
// `Vec::new()` sig to accept trailing for 28 method names.

const d = new Date(2026, 5, 20, 14, 30, 45, 100);

// 0-arg getters — trailing dropped.
console.log(d.getFullYear());
console.log(d.getFullYear("extra"));
console.log(d.getMonth());
console.log(d.getMonth(99, "ignored"));
console.log(d.getDate());
console.log(d.getDate("extra"));
console.log(d.getHours());
console.log(d.getHours(1, 2, 3));
console.log(d.getMinutes());
console.log(d.getMinutes("x"));
console.log(d.getSeconds());
console.log(d.getMilliseconds());
console.log(d.getMilliseconds("y", "z"));
console.log(d.getDay());
console.log(d.getDay(42));

// getTime / valueOf — trailing dropped.
console.log(d.getTime());
console.log(d.getTime("extra"));
console.log(d.valueOf());
console.log(d.valueOf(99));

// Format methods — trailing dropped.
console.log(d.toISOString());
console.log(d.toISOString("extra"));
console.log(d.toJSON());
console.log(d.toJSON("key"));
