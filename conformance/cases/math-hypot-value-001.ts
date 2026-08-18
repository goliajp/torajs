// §21.3.2.18 — Math.hypot as a VALUE: the call lane inlines sum²+sqrt
// at SSA level, but `.length` / `.name` / a bound reference need the
// ns-static reified cell. The runtime fold keeps the spec's steps 3-4
// ordering: any infinite argument answers +Infinity even when another
// is NaN.
console.log((Math.hypot as any).length, (Math.hypot as any).name);
const h: any = Math.hypot;
console.log(h(3, 4));
console.log(h());
console.log(h(-5));
console.log(h(3, 4, 12));
console.log(h(Infinity, NaN));
console.log(h(NaN, 1));
console.log(Math.hypot(3, 4));
