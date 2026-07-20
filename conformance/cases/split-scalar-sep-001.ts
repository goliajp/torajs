// §22.1.3.23 step 15 — a statically scalar separator (number /
// bool literal) is ToString'd: `s.split(123)` boxes the scalar and
// rides the runtime three-way dispatch instead of feeding raw bits
// to the (Str, Str) kernel (the S15.5.4.14_A2 exit-139 family).
const s = "this123is123a123string123object";
const r: any = s.split(123);
console.log(JSON.stringify(r), r.constructor === Array);
console.log(JSON.stringify("a1b1c1d".split(1, 2)));
console.log(JSON.stringify("xtruey".split(true)));
console.log(JSON.stringify("a0.5b".split(0.5)));
console.log(JSON.stringify("q".split(123)));
