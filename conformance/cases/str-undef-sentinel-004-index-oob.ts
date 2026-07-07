// string index OOB reads answer undefined — Substr-shaped immortal
// sentinel (RFC 20260707 residual chunk: view walks to "undefined"
// for ToString consumers; identity for strict/loose eq, typeof,
// truthy, JSON via to_owned propagation).
const s = "ab";
// print / strict-eq / loose-eq / typeof / truthy
console.log(s[5]);
console.log(s[5] === undefined);
console.log(s[5] === null);
console.log(s[5] == null);
console.log(typeof s[5]);
console.log(s[5] ? "truthy" : "falsy");
// content-eq must be identity, not text
console.log(s[5] === "undefined");
// negative index
console.log(s[-1]);
console.log(s[-1] === undefined);
// hit lane stays a real single-char view
console.log(s[1]);
console.log(s[1] === "b");
console.log(typeof s[1]);
console.log(s[1] ? "truthy" : "falsy");
// undefined === undefined across producers (exec miss vs index OOB)
const m = /a(b)?/.exec("a");
if (m !== null) {
  console.log(s[5] === m[1]);
}
// view-of-view index OOB (Substr receiver)
const v = s.slice(0, 1);
console.log(v[3]);
console.log(v[3] === undefined);
console.log(v[0]);
// concat / template read the sentinel text like bun stringifies undefined
console.log("x" + s[5]);
console.log(`${s[5]}!`);
// charAt / at / slice method family per spec
console.log(JSON.stringify(s.charAt(5)));
console.log(s.at(5));
console.log(s.at(5) === undefined);
console.log(s.at(-1));
console.log(JSON.stringify(s.slice(5, 6)));
// switch-on-string with an OOB scrutinee lands default
switch (s[9]) {
  case "a":
    console.log("case-a");
    break;
  default:
    console.log("case-default");
}
