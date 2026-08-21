// The split kernel's hot lane — a Latin-1 string cut on one byte —
// records the separator positions in one pass and fills the cells
// from them; anything it cannot hold (more than 64 separators, a
// string longer than a byte position can address) or that is not
// that shape (UTF-16 on either side, multi-byte or empty separator)
// takes the general two-pass build. Every edge of the lane and every
// hand-off to the general build, read back on the paths that decode
// the product. Rotation 469.

function show(tag: string, a: string[]) {
  console.log(tag, a.length, JSON.stringify(a), a.join("|"), a[0] === "" , a[a.length - 1] === "");
}

show("plain", "3 4 + 2 * 5 +".split(" "));
show("empty-mid", "a,,b".split(","));
show("lead-trail", ",abc,".split(","));
show("only-sep", ",".split(","));
show("two-seps", ",,".split(","));
show("no-match", "abc".split(","));
show("one-char", "a".split("a"));
show("one-char-miss", "a".split("b"));
show("trailing-run", "x;;;".split(";"));
show("leading-run", ";;;x".split(";"));

// exactly 64 separators: the last shape the lane holds
let s64 = "";
for (let i = 0; i < 64; i++) s64 = s64 + i + ",";
s64 = s64 + "end";
show("seps-64", s64.split(","));

// 65 separators: handed to the general build
let s65 = "";
for (let i = 0; i < 65; i++) s65 = s65 + "v" + i + ",";
show("seps-65", s65.split(","));

// 300 bytes with a few separators: longer than a byte position
let long = "";
for (let i = 0; i < 30; i++) long = long + "0123456789";
long = long + "|tail";
const lp = long.split("|");
console.log("long", lp.length, lp[0].length, lp[1]);

// heap parent (the rc-counted lane), then the parent dies first
function viaHeap(): string[] {
  let h = "p q r" + "!";
  return h.split(" ");
}
const kept = viaHeap();
let junk: string[] = [];
for (let i = 0; i < 64; i++) junk.push("zz" + i);
show("heap-parent", kept);

// UTF-16 haystack with a one-byte separator: the general build
show("utf16", "α β γ".split(" "));
// Latin-1 haystack with a UTF-16 separator: no match possible
show("latin1-utf16sep", "a b".split("β"));
// multi-byte and empty separators
show("multi", "a--b--c".split("--"));
show("empty-sep", "abc".split(""));
