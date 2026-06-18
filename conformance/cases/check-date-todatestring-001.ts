// S160 — `Date.prototype.toDateString()` per ES §21.4.4.35. Returns a
// human-readable date-only summary in the canonical "DDD MMM dd YYYY"
// shape (e.g. `Thu Jan 01 1970`). Mirrors the existing tora-side
// timezone substrate — toDateString uses `localtime_decompose` like
// `getFullYear`/`getMonth`/`getDate`/`getDay` do, so the formatted
// day-of-week / month / day match bun on the same host timezone.

// Epoch.
console.log(new Date(0).toDateString());

// A Friday in mid-year.
console.log(new Date(2026, 5, 19).toDateString());

// Year boundary — Saturday Jan 1 2000.
console.log(new Date(2000, 0, 1).toDateString());

// Year boundary — Friday Dec 31 1999.
console.log(new Date(1999, 11, 31).toDateString());

// Single-digit month + day padding (zero-padded to 2).
console.log(new Date(2026, 0, 1).toDateString());

// Far-future month.
console.log(new Date(2100, 11, 31).toDateString());
