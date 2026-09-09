// `recv[key]++` over an `any` receiver ran the key through
// ToPropertyKey — but a Number key reaches that lane unboxed, and the
// coercion read the bare integer 0 as the null box: `o[0]++` read and
// wrote `o["null"]` (the 5 stayed 5, a "null" key appeared, an array
// receiver went on to crash). String keys were fine, which is why this
// was filed as "the keyed lane does not increment a Map's expando".
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", f()); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name); }
}
t("obj[0]++", () => { const o: any = {}; o[0] = 5; const old = o[0]++; return [old, o[0], o["0"], Object.keys(o)]; });
t("obj[1]--", () => { const o: any = {}; o[1] = 5; const old = o[1]--; return [old, o[1], Object.keys(o)]; });
t("obj[k]++ k:any", () => { const o: any = {}; const k: any = 2; o[k] = 1; o[k]++; return [o[2], Object.keys(o)]; });
t("obj[1.5]++", () => { const o: any = {}; o[1.5] = 1; o[1.5]++; return [o[1.5], o["1.5"], Object.keys(o)]; });
t("obj['a']++", () => { const o: any = {}; o["a"] = 5; o["a"]++; return [o.a, Object.keys(o)]; });
t("obj[0] missing ++", () => { const o: any = {}; const old = o[0]++; return [old, o[0], Object.keys(o)]; });
t("arr[1]++", () => { const a: any = [1, 2]; const old = a[1]++; return [old, a[1], JSON.stringify(a)]; });
t("arr['1']++", () => { const a: any = [1, 2]; a["1"]++; return JSON.stringify(a); });
t("arr[5]++ oob", () => { const a: any = [1]; const old = a[5]++; return [old, a[5], a.length]; });
t("map[0]++", () => { const m: any = new Map(); m[0] = 5; m[0]++; return [m[0], Object.keys(m)]; });
t("set[1]+=3", () => { const s: any = new Set(); s[1] = 2; s[1] += 3; return s[1]; });
t("date[0]++", () => { const d: any = new Date(0); d[0] = 1; d[0]++; return d[0]; });
t("str value ++", () => { const o: any = {}; o[0] = "7"; const old = o[0]++; return [old, o[0], typeof o[0]]; });
t("bigint value ++", () => { const o: any = {}; o[0] = 7n; const old = o[0]++; return [old, o[0]]; });
t("null recv", () => { const o: any = null; o[0]++; return "no"; });
t("frozen recv", () => { const o: any = Object.freeze({ 0: 1 }); o[0]++; return o[0]; });
