// §7.1.17 step 9 — ToString of a function value is ToPrimitive over the
// function OBJECT: its own toString / valueOf decide before the source
// text. Covers the typed FnSig slot, the closure slot, the `+` position
// (default hint: valueOf first) and a non-callable own toString that
// has to be skipped (§7.1.1.1 step 5).
function f() { return 1; }
f.toString = function () { return "TS"; };
console.log(String(f), `${f}`, f + "", "p" + f);

function g() { return 2; }
(g as any).valueOf = function () { return "VO"; };
(g as any).toString = undefined;
console.log(String(g), g + "");
const ga: any = g;
console.log(ga.valueOf(), String(ga));

function h() { return 3; }
(h as any).valueOf = function () { return 7; };
console.log(h + "", `${h}`.length > 5);

const k = (x: number) => x + 1;
(k as any).toString = () => "arrow";
console.log(String(k), k + "!");

const cnt = { n: 0 };
function w() { return 4; }
(w as any).toString = function () { cnt.n++; return {}; };
(w as any).valueOf = function () { return true; };
console.log(new String(w) + "", cnt.n);

function z() { return 5; }
(z as any).toString = function () { return {}; };
(z as any).valueOf = function () { return {}; };
try {
  String(z);
  console.log("no throw");
} catch (e) {
  console.log((e as any).constructor.name);
}
try {
  console.log("x" + z);
} catch (e) {
  console.log((e as any).constructor.name);
}
