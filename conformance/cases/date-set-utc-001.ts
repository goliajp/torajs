// Date setUTC{FullYear,Month,Date,Hours,Minutes,Seconds,Milliseconds}
// setter family — typed tier + any-tier dispatch + reflection face;
// pure-UTC recompose (no DST pass, no two-digit-year mapping) with
// month/day/time overflow carry per spec MakeDay/MakeTime.
const d = new Date(1584275696789); // 2020-03-15T12:34:56.789Z
console.log(d.setUTCFullYear(2021));
console.log(d.toISOString());
console.log(d.setUTCMonth(0));
console.log(d.toISOString());
console.log(d.setUTCDate(28));
console.log(d.toISOString());
console.log(d.setUTCHours(3));
console.log(d.toISOString());
console.log(d.setUTCMinutes(7));
console.log(d.toISOString());
console.log(d.setUTCSeconds(9));
console.log(d.toISOString());
console.log(d.setUTCMilliseconds(1));
console.log(d.toISOString());
// multi-arg + overflow-normalize forms
const e = new Date(0);
console.log(e.setUTCFullYear(2020, 13, 32)); // → 2021-03-04 (month/day carry)
console.log(e.toISOString());
console.log(e.setUTCHours(25, 61, 61, 1001)); // time carry
console.log(e.toISOString());
// no two-digit-year mapping on the setter family
const f = new Date(0);
console.log(f.setUTCFullYear(50));
console.log(f.getUTCFullYear());
// getter round-trip
const g = new Date(0);
g.setUTCMonth(6, 4);
console.log(g.getUTCMonth(), g.getUTCDate());
// any-tier dispatch
const a: any = new Date(1584275696789);
console.log(a.setUTCDate(1));
console.log(a.toISOString());
console.log(a.setUTCHours(0, 0));
console.log(a.toISOString());
// own-property + reflection face (rides RFC 20260712 machinery)
console.log(Object.prototype.hasOwnProperty.call(Date.prototype, "setUTCDate"));
const dd: any = Object.getOwnPropertyDescriptor(Date.prototype, "setUTCHours");
console.log(typeof dd.value, dd.writable, dd.enumerable, dd.configurable);
console.log((a.setUTCMinutes as any).name, (a.setUTCMinutes as any).length);
