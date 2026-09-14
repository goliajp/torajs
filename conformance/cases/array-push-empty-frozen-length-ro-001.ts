// §23.1.3.23 step 6 — `push()` with no arguments still performs
// Set(O, "length", len, true); a frozen array or a non-writable length
// refuses it with a TypeError, and nothing changes.
function probe(label: string, a: any) {
  try {
    console.log(label, "ret", a.push());
  } catch (e) {
    console.log(label, (e as any).constructor === TypeError, a.length, a.hasOwnProperty(0));
  }
}
var frozen: any = [];
Object.freeze(frozen);
probe("frozen-var", frozen);
var ro: any = [];
Object.defineProperty(ro, "length", { writable: false });
probe("ro-var", ro);
var sealed: any = [1];
Object.seal(sealed);
probe("sealed-var", sealed);
var plain: any = [1, 2];
probe("plain-var", plain);
// typed receivers take the static lane
const tf: number[] = [];
Object.freeze(tf);
try { console.log("typed-frozen ret", tf.push()); } catch (e) { console.log("typed-frozen", (e as any).constructor === TypeError, tf.length); }
const tr: number[] = [3];
Object.defineProperty(tr, "length", { writable: false });
try { console.log("typed-ro ret", tr.push()); } catch (e) { console.log("typed-ro", (e as any).constructor === TypeError, tr.length); }
const tp: number[] = [4, 5, 6];
console.log("typed-plain", tp.push(), tp.length);
