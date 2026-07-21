// FnSig-lane `decl.prototype` member-base promotion (G9) — the read
// rides the canonical __fncell_ closure singleton, so expando
// writes on it are visible through every lane; name/length keep
// their static reflection arms and direct calls stay raw FnSig.
function decl() { return 1; }
console.log(typeof decl.prototype);
(decl.prototype as any).tag = "p";
let a: any = decl;
console.log(typeof a.prototype, a.prototype.tag);
console.log(decl(), decl.name, decl.length);
function two(x: number, y: number) { return x + y; }
console.log(typeof two.prototype, two(1, 2), two.name, two.length);
let e = function () { return 2; };
console.log(typeof e.prototype, e());
