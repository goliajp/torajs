// `xs[1.5] = v` names the property "1.5", never element 1 (§7.1.19):
// the spec creates that property on the array and leaves the elements
// and the length alone. The typed store truncated the index and
// overwrote xs[1]. An F64 index now carries the "not an integer" flag
// the read side got one commit back, and the flagged store takes the
// keyed kernel, which makes the expando. (A frozen receiver's expando
// refusal is the kernel's, registered separately.)
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", f()); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name); }
}
t("write 1.5", () => { const xs: number[] = [7.5, 8.5, 9.5]; xs[1.5] = 100; return [JSON.stringify(xs), xs.length, xs[1], (xs as any)["1.5"], xs[1.5]]; });
t("write 2.0", () => { const xs: number[] = [7.5, 8.5, 9.5]; xs[2.0] = 1; return [JSON.stringify(xs), xs.length]; });
t("write -0.5", () => { const xs: number[] = [7.5]; xs[-0.5] = 3; return [JSON.stringify(xs), xs[0], (xs as any)["-0.5"], Object.keys(xs)]; });
t("write len/2 odd", () => { const xs: number[] = [1.5, 2.5, 3.5]; const h = xs.length / 2; xs[h] = 0; return [JSON.stringify(xs), Object.keys(xs)]; });
t("write len/2 even", () => { const xs: number[] = [1.5, 2.5]; const h = xs.length / 2; xs[h] = 0; return [JSON.stringify(xs), xs.length]; });
t("str[] 0.5", () => { const ss: string[] = ["a", "b"]; ss[0.5] = "z"; return [JSON.stringify(ss), (ss as any)["0.5"], Object.keys(ss)]; });
t("any[] 0.5", () => { const a: any[] = [1, 2]; a[0.5] = "q"; return [JSON.stringify(a), a[0.5], Object.keys(a)]; });
t("NaN / Infinity", () => { const xs: number[] = [1.5]; xs[NaN] = 2; xs[Infinity] = 3; return [JSON.stringify(xs), (xs as any)["NaN"], (xs as any)["Infinity"], Object.keys(xs)]; });
t("compound", () => { const xs: number[] = [1.5, 2.5]; xs[0.5] = 1; xs[0.5] += 4; return [JSON.stringify(xs), xs[0.5]]; });
t("in loop", () => { const xs: number[] = [0.5, 0.5, 0.5]; for (let i = 0; i < 3; i += 0.5) xs[i] = i; return [JSON.stringify(xs), Object.keys(xs)]; });
