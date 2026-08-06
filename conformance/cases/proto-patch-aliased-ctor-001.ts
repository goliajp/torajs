// A builtin prototype can be reached through a name the scan does not
// recognise. `const A = Array` hands the constructor value to `A`, and
// `A.prototype.join = f` is then a patch spelled entirely in names the
// module-level scan has never heard of — so before this was closed the
// program fell to L0, the typed tier stayed open, and the patch was
// IGNORED. Not rejected, not slower: ignored, with the kernel's answer
// printed as if nothing had been assigned.
//
// The scan cannot follow the value, so it does the one sound thing it
// can: a builtin constructor mentioned anywhere other than "read
// something through it" stands its whole family down. Which family is
// known from the escaped name, so this costs Array's typed tier in
// this program and nothing else.

const A: any = Array;
(A.prototype as any).join = function () {
  return "PATCHED";
};
const xs: number[] = [1, 2, 3];
console.log(String(xs.join("-")));

// Passing the constructor to a function is the same escape: the callee
// could patch it, and the scan cannot see inside.
function patch(C: any): void {
  C.prototype.toUpperCase = function () {
    return "ALSO PATCHED";
  };
}
patch(String);
const s: string = "abc";
console.log(s.toUpperCase());

// Reads THROUGH the name are not escapes, and must keep their ordinary
// answers — these are the shapes that would make the rule expensive if
// it over-fired. `new C(n)` and `x instanceof C` do not even produce a
// name expression in the AST; `typeof` and a static call do, in
// positions that hand the value to no one.
console.log(String(new Array(2).length));
console.log(String(xs instanceof Array));
console.log(typeof Array);
console.log(String(Array.isArray(xs)));
console.log(String(Number.isInteger(3)));
