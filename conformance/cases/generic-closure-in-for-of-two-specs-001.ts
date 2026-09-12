// A closure constructed inside a for-of body of an implicitly-generic
// function, with the function instantiated twice (a direct call and a
// value read). The spec body clone used to keep the template's for-of
// ExprIds, so the closure inside was never cloned per specialization:
// one lifted decl served both, typed by whichever spec checked last —
// `typeof thisv` answered `key`'s `string` (rotation 637, 637-06).
function callIt(f) { return f(); }
function tC(thisv) { for (var key of ["charAt"]) { return callIt(() => typeof thisv + "|" + (thisv === null) + "|" + key); } return "none"; }
function tD(thisv) { var out = ""; for (var key of ["charAt", "at"]) { out += callIt(() => typeof thisv + "|" + key + ";"); } return out; }
var zc: any = tC;
var zd: any = tD;
console.log(tC(null), tD(null));
console.log(zc(undefined), zd(3));
console.log(tC("s"), tD({}));
