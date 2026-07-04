// RFC 20260704 C4-3a — Date methods through `any` receivers.
const d: any = new Date(0);
console.log(d.getTime());
console.log(d.valueOf());
console.log(d.getUTCFullYear());
console.log(d.getUTCMonth());
console.log(d.getUTCDate());
console.log(d.getUTCHours());
console.log(d.getUTCMinutes());
console.log(d.getUTCSeconds());
console.log(d.getUTCMilliseconds());
console.log(d.getUTCDay());
console.log(d.toISOString());
console.log(d.toJSON());
console.log(d.toUTCString());
console.log(d.getFullYear());
console.log(d.getMonth());
console.log(d.getDate());
console.log(d.getHours());
console.log(d.getMinutes());
console.log(d.getSeconds());
console.log(d.getMilliseconds());
console.log(d.getDay());
console.log(d.setTime(86400000));
console.log(d.getUTCDate());
const e: any = new Date(0);
console.log(e.setFullYear(2020));
console.log(e.getUTCFullYear());
console.log(e.setFullYear(2021, 5, 15));
console.log(e.toISOString());
console.log(e.setMilliseconds(250));
console.log(e.getMilliseconds());
try {
  e.notADateMethod();
} catch (err) {
  console.log("threw");
}
