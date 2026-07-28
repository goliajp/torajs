// S2.27 regression pin — a TYPED-param setter behind the define
// kernel. accessor_param_kind used to read sig param 1 assuming an
// env-first signature, but fn_sigs is the user face: the read answered
// None → ACC_KIND_ANY, the invoke handed the NaN-box verbatim, and an
// i64-ABI body printed box bits (-562949953421305 for `= 7`).
var order: string[] = [];
var o: any = {};
Object.defineProperty(o, "b", { set(v: number) { order.push("set:" + v); } });
o.b = 7;
console.log(order.join(","));

// f64-ABI setter body — the arithmetic keeps the param in the F64
// register class, exercising the ACC_KIND_F64 unbox arm.
var got = 0;
var p: any = {};
Object.defineProperty(p, "w", { set(x: number) { got = x * 0.5; } });
p.w = 9;
console.log(got);

// typed-ret getter alongside — the ret-kind half was always correct;
// pin the pair shape end to end.
var q: any = {};
Object.defineProperty(q, "g", {
  get() { return 21; },
  set(v: number) { order.push("q:" + v); }
});
console.log(q.g);
q.g = 3;
console.log(order.join(","));
