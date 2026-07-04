// L3b #11 residue (chunk 529) — Function-as-Object expandos read
// and written through any: the member gate's Closure arm probes
// the lazy props_dynobj (T-27), first write allocates it, and both
// directions round-trip whether the expando was set on the typed
// face or the any face. Absent keys answer undefined.
const f = (x: number) => x * 2;
(f as any).meta = "closure-prop";
const g: any = f;
console.log(g.meta);
console.log(g.missing);
g.viaAny = 42;
console.log(g.viaAny);
console.log((f as any).viaAny);
g.meta = "updated";
console.log(g.meta);
const h: any = (y: number) => y + 1;
console.log(h.never);
h.first = true;
console.log(h.first);
console.log(h(4));
for (let i = 0; i < 12; i++) {
  g["p" + i] = i;
}
console.log(g["p0"]);
console.log(g["p11"]);
console.log(g.meta);
