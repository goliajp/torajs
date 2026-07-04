// RFC 20260704 C4-3c-2 — RegExp property reads through `any`
// receivers.
const r: any = /ab+c/gi;
console.log(r.source);
console.log(r.flags);
console.log(r.lastIndex);
console.log(r.global);
console.log(r.ignoreCase);
console.log(r.multiline);
console.log(r.dotAll);
console.log(r.unicode);
console.log(r.sticky);
r.test("xxabcyy");
console.log(r.lastIndex);
const s: any = /a.c/su;
console.log(s.flags);
console.log(s.dotAll);
console.log(s.unicode);
console.log(s.global);
const o: any = { source: "user", flags: 7 };
console.log(o.source);
console.log(o.flags);
const a: any = [1, 2];
console.log(a.source);
