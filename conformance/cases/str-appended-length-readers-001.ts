// A string something appended to carries a capacity in the four
// bytes above its length. Everything that reads that length has to
// read the u32 it actually is — a u64 read picks up the capacity
// with it and answers a number in the billions, which is how
// `BigInt(binaryString)` came to segfault after a 128-round build.
let s = "0b1";
for (let i = 0; i < 128; i++) s += "0";
console.log(s.length, BigInt(s) === 340282366920938463463374607431768211456n);
let d = "1";
for (let i = 0; i < 40; i++) d += "0";
console.log(BigInt(d).toString().length, Number(d) > 0);
let k = "k";
for (let i = 0; i < 40; i++) k += "x";
const m = new Map<string, number>();
m.set(k, 7);
const st = new Set<string>();
st.add(k);
console.log(m.get(k), st.has(k));
let j = '{"a';
for (let i = 0; i < 40; i++) j += "b";
j += '":1}';
const parsed = JSON.parse(j);
console.log(parsed["a" + "b".repeat(40)]);
let e = "";
for (let i = 0; i < 40; i++) e += "z";
console.log(e ? "truthy" : "falsy", (e + "!").length);
let sym = "s";
for (let i = 0; i < 40; i++) sym += "y";
console.log(String(Symbol(sym)).length);
let er = "boom";
for (let i = 0; i < 40; i++) er += "!";
try {
  throw new Error(er);
} catch (x) {
  console.log((x as Error).message.length);
}
