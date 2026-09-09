// A Number key on an `any` receiver rode the integer index lane after
// a truncating coercion, so `o[1.5]` read and wrote `o[1]` — the two
// are different properties ("1.5" is not an array index). A key whose
// value is not provably integral now takes the keyed kernel boxed as
// the double it is; the kernel routes an integral double to the index
// lane itself and ToStrings the rest, which is what ToPropertyKey says.
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", f()); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name); }
}
t("obj write/read 1.5", () => { const o: any = {}; o[1.5] = "a"; return [o[1.5], o[1], o["1.5"], Object.keys(o)]; });
t("obj 1.5 vs 1", () => { const o: any = {}; o[1] = "int"; o[1.5] = "half"; return [o[1], o[1.5], Object.keys(o)]; });
t("obj -1.5", () => { const o: any = {}; o[-1.5] = 1; return [o[-1.5], o[-1], Object.keys(o)]; });
t("obj key from f64 local", () => { const o: any = {}; const k = 2.5; o[k] = 3; return [o[k], o[2], Object.keys(o)]; });
t("obj key 2.0", () => { const o: any = {}; o[2.0] = 3; return [o[2], o["2"], Object.keys(o)]; });
t("obj 1.5++", () => { const o: any = {}; o[1.5] = 1; o[1.5]++; return [o[1.5], Object.keys(o)]; });
t("obj 1.5 += ", () => { const o: any = {}; o[1.5] = 1; o[1.5] += 2; return [o[1.5], Object.keys(o)]; });
t("arr 1.5", () => { const a: any = [7, 8]; a[1.5] = 9; return [a[1.5], a[1], a.length, JSON.stringify(a), Object.keys(a)]; });
t("arr read 0.5 missing", () => { const a: any = [7]; return [a[0.5], a[0]]; });
t("map 0.5", () => { const m: any = new Map(); m[0.5] = 1; return [m[0.5], m[0], Object.keys(m)]; });
t("optional 1.5", () => { const o: any = { "1.5": "x" }; return [o?.[1.5], o?.[1]]; });
t("NaN key", () => { const o: any = {}; o[NaN] = 1; return [o[NaN], o["NaN"], Object.keys(o)]; });
t("Infinity key", () => { const o: any = {}; o[Infinity] = 1; return [o[Infinity], Object.keys(o)]; });
t("large 1e21", () => { const o: any = {}; o[1e21] = 1; return Object.keys(o); });
