// invalid-date substrate probe — RFC 20260713-date-invalid-time
const inv = new Date(NaN);
console.log(inv.getTime(), inv.getFullYear(), inv.getUTCMonth(), inv.getDay(), inv.getTimezoneOffset());
console.log(inv.toString());
console.log(inv.toDateString());
console.log(inv.toUTCString());
console.log(inv);
try { inv.toISOString(); } catch (e) { console.log((e as Error).name, (e as Error).message); }
// ctor clip
console.log(new Date(8.64e15).getTime(), new Date(8.64e15 + 1).getTime());
console.log(new Date("not a date").getTime());
// setter NaN + clip + revive
const d = new Date(2020, 0, 2, 3, 4, 5, 6);
console.log(d.setDate(NaN), d.getTime());
console.log(d.setFullYear(2021), d.getFullYear(), d.getMonth(), d.getDate());
const m = new Date(8.64e15);
console.log(m.setDate(28), m.getTime());
// setter keep vs supplied
const k = new Date(2020, 5, 15, 12, 30, 45, 500);
k.setHours(5);
console.log(k.getHours(), k.getMinutes(), k.getSeconds(), k.getMilliseconds());
// Date.UTC NaN + valid
console.log(Date.UTC(NaN), Date.UTC(2020, 0, 1));
// negative year serialization
const neg = new Date(-1, 0, 1);
console.log(neg.toDateString());
// any-world invalid date
const a: any = new Date(NaN);
console.log(a.getDate(), a.toDateString());
console.log([new Date(NaN), new Date(0)]);
// valueOf NaN propagation
console.log(inv.valueOf());
