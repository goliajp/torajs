// Date per-field setters per ES §21.4.4.20-26 —
// setFullYear / setMonth / setDate / setHours / setMinutes /
// setSeconds / setMilliseconds. Each takes 1-N Number args
// (trailing positions optional) and returns the new d.ms.

// setFullYear — 1/2/3 arg forms
const a1 = new Date(0); a1.setFullYear(2024); console.log("setFullYear_y", a1.getTime(), a1.toISOString());
const a2 = new Date(0); a2.setFullYear(2024, 5); console.log("setFullYear_ym", a2.getTime(), a2.toISOString());
const a3 = new Date(0); a3.setFullYear(2024, 5, 15); console.log("setFullYear_ymd", a3.getTime(), a3.toISOString());

// setMonth — 1/2 arg forms
const b1 = new Date(0); b1.setMonth(5); console.log("setMonth_m", b1.getTime(), b1.toISOString());
const b2 = new Date(0); b2.setMonth(5, 15); console.log("setMonth_md", b2.getTime(), b2.toISOString());

// setDate
const c1 = new Date(0); c1.setDate(15); console.log("setDate", c1.getTime(), c1.toISOString());

// setHours — 1/2/3/4 arg forms
const d1 = new Date(0); d1.setHours(15); console.log("setHours_h", d1.getTime(), d1.toISOString());
const d2 = new Date(0); d2.setHours(15, 30); console.log("setHours_hm", d2.getTime(), d2.toISOString());
const d3 = new Date(0); d3.setHours(15, 30, 45); console.log("setHours_hms", d3.getTime(), d3.toISOString());
const d4 = new Date(0); d4.setHours(15, 30, 45, 678); console.log("setHours_hmsms", d4.getTime(), d4.toISOString());

// setMinutes — 1/2/3 arg forms
const e1 = new Date(0); e1.setMinutes(45); console.log("setMinutes_m", e1.getTime(), e1.toISOString());
const e2 = new Date(0); e2.setMinutes(45, 30); console.log("setMinutes_ms", e2.getTime(), e2.toISOString());
const e3 = new Date(0); e3.setMinutes(45, 30, 678); console.log("setMinutes_msms", e3.getTime(), e3.toISOString());

// setSeconds — 1/2 arg forms
const f1 = new Date(0); f1.setSeconds(45); console.log("setSeconds_s", f1.getTime(), f1.toISOString());
const f2 = new Date(0); f2.setSeconds(45, 678); console.log("setSeconds_sms", f2.getTime(), f2.toISOString());

// setMilliseconds
const g1 = new Date(0); g1.setMilliseconds(678); console.log("setMilliseconds", g1.getTime(), g1.toISOString());

// Normalize rollover — Feb 30 → Mar 2
const h1 = new Date(0); h1.setMonth(1, 30); console.log("rollover_feb30", h1.getTime(), h1.toISOString());

// Hour overflow (25 → next day 01:00)
const h2 = new Date(0); h2.setHours(25); console.log("rollover_h25", h2.getTime(), h2.toISOString());

// Negative day (setDate(0) → prior month last day)
const h3 = new Date(Date.UTC(2024, 5, 15)); h3.setDate(0); console.log("date_zero", h3.getTime(), h3.toISOString());

// Setter return value is the new d.ms
const r1 = new Date(0);
const rv = r1.setFullYear(2024, 5, 15);
console.log("ret_value", rv, r1.toISOString());
