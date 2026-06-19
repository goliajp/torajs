// S168 — `Date.prototype.getTimezoneOffset()` per ES §21.4.4.7.
// Returns minutes BEHIND UTC for the receiver's instant; JS sign
// flips libc's seconds-ahead-of-UTC. Asia/Tokyo (UTC+9, no DST) →
// `-540` for every instant. Test fixture pins both the convention
// (3 instants spanning years) and the integration with sibling
// local-time getters (`getHours` etc. must read off the same local
// offset, otherwise rendering subtly desyncs).

// 1) summer-day mid-year instant.
const a = new Date(2026, 5, 19, 10, 0, 0);
console.log(a.getTimezoneOffset());

// 2) January 1st new-year instant.
const b = new Date(2026, 0, 1, 0, 0, 0);
console.log(b.getTimezoneOffset());

// 3) UNIX epoch (UTC midnight, just past Asia/Tokyo morning).
const c = new Date(0);
console.log(c.getTimezoneOffset());

// 4) Negative-ms instant (1969 pre-epoch).
const d = new Date(-1000);
console.log(d.getTimezoneOffset());

// 5) Sibling local-time getters integration regression — local
// hour must compose with the offset to recover the UTC value.
const e = new Date(2026, 5, 19, 10, 0, 0);
const localHours = e.getHours();
const offsetMin = e.getTimezoneOffset();
const utcHours = e.getUTCHours();
const localToUtcAdjusted = ((localHours * 60 + offsetMin) + 24 * 60) % (24 * 60);
console.log(localToUtcAdjusted === utcHours * 60);
console.log(localHours);
console.log(utcHours);
console.log(offsetMin);

// 6) typeof — must be number.
console.log(typeof a.getTimezoneOffset());
