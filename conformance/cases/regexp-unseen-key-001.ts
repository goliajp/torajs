// A method called by a key computed at run time can be a String
// method that hands its RegExp argument to the RegExp protocol
// (§22.1.3.13 step 2, §22.1.3.14 step 2): `s[k](re)` with `k` spelled
// nowhere as one string. The link cannot see the method id, so it
// keeps the protocol algorithms; @@matchAll needs them even for a
// plain RegExp (§22.2.6.9 answers a lazy RegExp String Iterator).
const parts = ["mat", "ch"];
const s: any = "xabyab";
const k = parts.join("");
const m = s[k](/a(b)?/);
console.log(m[0], m[1], m.index);
const it: any = s[k + "All"](/a(b)/g);
console.log(Object.prototype.toString.call(it));
for (const r of it) console.log(r[0], r[1], r.index);
const sp = ["sp", "lit"].join("");
console.log(s[sp](/y/).join("|"));
