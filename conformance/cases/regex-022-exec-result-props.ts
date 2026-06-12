// Regex exec/match result properties — spec §22.2.7.8: the match
// array carries `index` (match start), `input` (subject string,
// rc-shared) and `groups` (named-capture dict with null prototype,
// or undefined without named captures). Printed insertion-ordered
// by the arrprops face: `[ "abb", index: 1, input: "xabby",
// groups: undefined ]`.
//
// Also covers: non-participating capture elements print `undefined`
// (NULL slot — was rendered as ""), named groups print with the
// `[Object: null prototype] ` prefix, non-global s.match(re) shares
// the exec shape, and global exec advances index across calls.

const re = /ab*/;
const m = re.exec("xabby");
console.log(m);

const m2 = "xabby".match(/ab*/);
console.log(m2);

const re3 = /a(b)(c)?/;
console.log(re3.exec("zabq"));

const re4 = /(?<y>b)/;
console.log(re4.exec("abc"));

const reg = /b/g;
console.log(reg.exec("abcb"));
console.log(reg.exec("abcb"));
