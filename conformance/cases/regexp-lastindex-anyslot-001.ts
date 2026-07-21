// §22.2.4.1 — lastIndex is an ordinary {writable: true} data
// property: assignment stores ANY value verbatim (the tr cell keeps
// an f64 fast slot + a boxed overflow slot); ToLength happens only at
// the §22.2.7.2 exec-entry consumption. Covers the static hint lane,
// the dynamic-key lane (the t262 isWritable shape), and the gOPD
// verbatim readback.
let re: any = /a/g;
re.lastIndex = "abc";
console.log(re.lastIndex, typeof re.lastIndex);
let d: any = Object.getOwnPropertyDescriptor(re, "lastIndex");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(re.hasOwnProperty("lastIndex"));
console.log(JSON.stringify(re.exec("xxa")));
console.log(re.lastIndex, typeof re.lastIndex);
re.lastIndex = 2.9;
console.log(re.lastIndex);
// dynamic-key write + read (the harness isWritable shape)
let k: string = "lastIndex";
re[k] = "unlikelyValue";
console.log(re[k]);
re[k] = 7;
console.log(re.lastIndex);
// sticky consumption coerces the verbatim value through ToLength
let ry: any = /a/y;
ry.lastIndex = "1";
console.log(JSON.stringify(ry.exec("xa")));
console.log(ry.lastIndex);
// a non-global non-sticky match never touches lastIndex
let rn: any = /b/;
rn.lastIndex = "keep";
rn.exec("abc");
console.log(rn.lastIndex);
