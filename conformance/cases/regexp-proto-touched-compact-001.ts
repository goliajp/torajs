// Deletes followed by adds can bring %RegExp.prototype%'s entry
// counts back to what its mint left once a full entry array compacts
// its holes away; the user `exec` installed last must still be called.
let calls = 0;
const P = RegExp.prototype as any;
delete P[Symbol.search];
P.p1 = 1;
P.p2 = 2;
delete P.p1;
delete P.p2;
P.exec = function (s: string) {
  calls++;
  return null;
};
const s: any = "xax";
const re: any = /a/;
console.log(s.replace(re, "b"), calls);
console.log(s.match(re), calls);
console.log(s.split(re).length, calls);
