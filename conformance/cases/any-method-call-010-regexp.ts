// RFC 20260704 C4-3c-1 — RegExp methods through `any` receivers.
const r: any = /ab+/g;
console.log(r.test("xxabb"));
console.log(r.test("xyz"));
console.log(r.toString());
const m: any = r.exec("zabbz");
console.log(m[0]);
console.log(r.exec("zzz"));
const nr: any = /(\d+)-(\d+)/;
const m2: any = nr.exec("a 12-34");
console.log(m2[0], m2[1], m2[2]);
const sr: any = /^a.c$/s;
console.log(sr.test("a\nc"));
try {
  r.notARegexMethod();
} catch (err) {
  console.log("threw");
}
