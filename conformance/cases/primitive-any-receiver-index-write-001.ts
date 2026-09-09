// A keyed store on a primitive `any` receiver — `n[0] = 1`, `s[0] =
// "x"` — is §13.15.2 PutValue on a primitive base: [[Set]] on the
// wrapper finds no setter and refuses the data write on a non-object
// receiver (a String's own index is non-writable besides), and a
// module-strict program turns that into a TypeError. The member
// spelling (`n.x = 1`) already threw; the index kernel discarded the
// value silently on a sloppy-mode reading its doc cited. Every other
// cell here was already right and is asserted alongside.
function w(tag: string, f: () => void) { try { f(); console.log(tag, "NO-THROW"); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name); } }
const n: any = 1, s: any = "ab", b: any = true, sy: any = Symbol("s"), nu: any = null, un: any = undefined;
w("num[0]=1", () => { n[0] = 1; });
w("str[0]=x", () => { s[0] = "x"; });
w("str.length=1", () => { s.length = 1; });
w("bool.x=1", () => { b.x = 1; });
w("num.x=1", () => { n.x = 1; });
w("sym[0]=1", () => { sy[0] = 1; });
w("null[0]=1", () => { nu[0] = 1; });
w("undef.x=1", () => { un.x = 1; });
w("num[0]++", () => { n[0]++; });
w("str[5]=x oob", () => { s[5] = "x"; });
w("num[0]+=1", () => { n[0] += 1; });
const nn: any = 1; w("num[0] read", () => { console.log("  read", nn[0], nn["x"]); });
