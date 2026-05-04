// Phase 2.0b.2: component ctor + ISO parse + Date.parse + Date.UTC.

const d1 = new Date(2020, 0, 1);
console.log(d1.getFullYear(), d1.getMonth(), d1.getDate());

const d2 = new Date(2023, 10, 14, 22, 13, 20);
console.log(d2.getFullYear(), d2.getMonth(), d2.getDate());
console.log(d2.getHours(), d2.getMinutes(), d2.getSeconds());

const d3 = new Date(2020, 5, 15, 10, 30, 45, 678);
console.log(d3.getMilliseconds());

// JS year quirk
const d4 = new Date(99, 0, 1);
console.log(d4.getFullYear());

// ISO parse
const d5 = new Date("2020-01-01T00:00:00.000Z");
console.log(d5.getTime());

const d6 = new Date("2023-11-14T22:13:20Z");
console.log(d6.getTime());

// Date-only ISO is UTC
const d7 = new Date("2020-01-01");
console.log(d7.getUTCFullYear(), d7.getUTCMonth(), d7.getUTCDate());

console.log(Date.parse("2020-01-01T00:00:00.000Z"));
console.log(Date.UTC(2020, 0, 1, 0, 0, 0, 0));
console.log(Date.UTC(2023, 10, 14, 22, 13, 20, 0));

// ISO round-trip
const d8 = new Date(Date.UTC(2024, 5, 15, 10, 30, 45, 123));
console.log(d8.toISOString());
