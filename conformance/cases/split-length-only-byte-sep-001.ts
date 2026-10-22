// A measured split on a one-unit Latin-1 literal counts the byte in a
// Latin-1 receiver; every other receiver or separator keeps the
// general count. Lengths 0..40 cover the short and word-wise reads,
// longer runs cover the block reads and their tail.

// receivers built at run time, every length, the separator at each end
let lens: string = "";
for (let n: number = 0; n <= 40; n = n + 1) {
  let s: string = "";
  for (let k: number = 0; k < n; k = k + 1) {
    s = s + (k % 3 === 0 ? "," : "x");
  }
  const parts: string[] = s.split(",");
  const head: string[] = ("," + s).split(",");
  const tail: string[] = (s + ",").split(",");
  lens = lens + parts.length + "/" + head.length + "/" + tail.length + " ";
}
console.log(lens);

// edge shapes on literals
console.log("".split(",").length, ",".split(",").length, ",,".split(",").length);
console.log(",a,".split(",").length, "a,,,b".split(",").length, "abc".split(",").length);
console.log("3 4 + 2 * 5 +".split(" ").length, "\0a\0".split("\0").length, "a\0b".split("x").length);

// a Latin-1 byte above ASCII, on Latin-1 and on UTF-16 receivers
console.log("caféxéé".split("é").length, "ÿaÿ".split("ÿ").length, "日é本é".split("é").length);

// UTF-16 receivers with an ASCII separator take the general count
console.log("日 本 語".split(" ").length, "α,β,,γ,".split(",").length, "😀,😀".split(",").length);

// a separator that is not one Latin-1 unit
console.log("a→b→c".split("→").length, "a, b, c".split(", ").length, "日日本".split("日").length);

// long runs: block reads and a tail of every width
let longs: string = "";
for (let n: number = 16; n <= 300; n = n + 17) {
  let s: string = "";
  for (let k: number = 0; k < n; k = k + 1) {
    s = s + (k % 5 === 1 ? " " : "w");
  }
  const parts: string[] = s.split(" ");
  longs = longs + parts.length + " ";
}
console.log(longs);
let big: string = "";
for (let k: number = 0; k < 5000; k = k + 1) {
  big = big + ";";
}
const all: string[] = big.split(";");
console.log(all.length);

// receiver from argv-like runtime input through a function
function count(s: string): number {
  return s.split("_").length;
}
console.log(count("3_4_+_2_*_5_+"), count(""), count("_"), count("日_本"));
