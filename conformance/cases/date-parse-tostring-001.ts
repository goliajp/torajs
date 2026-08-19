// §21.4.3.2 — Date.parse must round-trip the toString and
// toUTCString shapes, not just ISO.
const d = new Date(2026, 7, 19, 12, 58, 39);
const ms = d.getTime();
console.log("rt-tostring", Date.parse(d.toString()) === ms);
console.log("rt-utcstring", Date.parse(d.toUTCString()) === ms);
const d2 = new Date(d.toString());
console.log("ctor-tostring", d2.getTime() === ms);
// @ts-ignore
console.log("datecall-close", Math.abs(Date.parse(Date()) - Date.now()) < 5000);
console.log("garbage", Number.isNaN(Date.parse("Wed Aug 19 2026 03:58:39 GMTx")));
