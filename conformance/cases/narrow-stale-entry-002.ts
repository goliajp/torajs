// an argument a call undid the narrowing of reaches a parameter the
// callee reads first, through shapes other than a plain read
type N = { n: number };
let q: N | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = { n: 1 };
}
let c = true;
function f(x: N): number { return x.n }
function viaTernary(p: N): number { return f(c ? p : p) }
function viaOr(p: N): number { return f(p || p) }
function viaAnd(p: N, o: N): number { return f(p && o) }
function viaNullish(p: N): number { return f(p ?? p) }
function viaComma(p: N): number { return f((0, p)) }
function viaAssign(p: N): number { let t: N = { n: 0 }; return f(t = p) }
function viaArrow(p: N): number { return f(((x: N) => x)(p)) }
run("ternary", () => { if (q !== null && nullQ()) console.log(viaTernary(q)) });
run("or", () => { if (q !== null && nullQ()) console.log(viaOr(q)) });
run("and", () => { if (q !== null && nullQ()) console.log(viaAnd(q, { n: 2 })) });
run("nullish", () => { if (q !== null && nullQ()) console.log(viaNullish(q)) });
run("comma", () => { if (q !== null && nullQ()) console.log(viaComma(q)) });
run("assign", () => { if (q !== null && nullQ()) console.log(viaAssign(q)) });
run("arrow", () => { if (q !== null && nullQ()) console.log(viaArrow(q)) });
const k: N = { n: 4 };
console.log(viaTernary(k), viaOr(k), viaAnd(k, { n: 5 }), viaNullish(k), viaComma(k), viaAssign(k), viaArrow(k));
