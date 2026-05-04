// Phase 2.0b: getX methods (local time + UTC variants).
// Local-time getters use libc localtime_r (honors TZ env);
// UTC variants use branch-free civil_from_days arithmetic.

const d = new Date(1577836800000);  // 2020-01-01T00:00:00.000Z
console.log(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
console.log(d.getUTCHours(), d.getUTCMinutes(), d.getUTCSeconds());
console.log(d.getUTCMilliseconds(), d.getUTCDay());

// Local-time depends on TZ env; assume bun + tr both pick the same.
const d2 = new Date(0);
console.log(d2.getUTCFullYear(), d2.getUTCMonth(), d2.getUTCDate());
console.log(d2.getUTCHours(), d2.getUTCMinutes());

// Pre-epoch UTC
const d3 = new Date(-1);
console.log(d3.getUTCFullYear(), d3.getUTCMonth(), d3.getUTCDate());
console.log(d3.getUTCHours(), d3.getUTCMinutes(), d3.getUTCSeconds(), d3.getUTCMilliseconds());

// Day-of-week (UTC)
const d4 = new Date(1577836800000);  // Wed Jan 1 2020
console.log(d4.getUTCDay());
