// RFC 20260711 follow-up — arr.join encoding-aware (the kernels were
// byte-blind vs the dual-encoding Str payloads: joining any UTF-16
// element / separator / Substr parent copied half the payload and
// re-tagged it Latin-1).
console.log(["x", "؜", "y"].join("|"));
console.log(["ab؜cd"].join(""));
console.log(["中", "文"].join("、"));
console.log(["a", "b"].join("—"));
console.log([1, 2, 3].join("・"));
console.log([1.5, NaN].join("÷"));
console.log([true, false].join("中"));
const anyArr: any[] = ["中", 42, null, undefined, "é"];
console.log(anyArr.join("+"));
console.log(["𝄞", "a"].join("𝄢"));
console.log([1234567].toLocaleString());
console.log([1234567.5, 2].toLocaleString());
const pushed: string[] = [];
pushed.push(String.fromCodePoint(0x61c));
pushed.push("z");
console.log(pushed.join(""), pushed.join("").length);
const s = "汉字界";
console.log([s.charAt(0), s.charAt(2)].join("·"));
const t = "abc";
console.log([t.charAt(1), t.charAt(2)].join("汉"));
console.log(["a", "b", "c"].join(""));
console.log([].join(","));
