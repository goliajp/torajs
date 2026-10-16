// An own `exec` on one RegExp: typed String methods given that RegExp run
// its protocol algorithm, whose RegExpExec calls the own `exec`; another
// RegExp in the same program keeps the builtin matcher.
const r = /a/;
(r as any).exec = function (s: string) {
  return null;
};
console.log("xax".replace(r, "b"));
console.log(JSON.stringify("xax".match(r)));
console.log("xax".search(r));
console.log("xax".replace(/[a]/, "b"));
const g = /a/g;
let seen = 0;
(g as any).exec = function (this: RegExp, s: string) {
  seen++;
  return seen === 1 ? RegExp.prototype.exec.call(/[a]/, s) : null;
};
console.log("xax".replace(g, "b"), seen);
