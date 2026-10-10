// `for (p of s.split(c))` with a one-byte literal separator scans a
// Latin-1 parent eight bytes at a time and cuts each part from the
// separator marks of the word last read. Covers parents of length
// 1..17 with the separator in every lane of the first three words,
// as the first and the last byte, repeated (empty parts), next to
// bytes that differ from it only in the high bit or the low bit, and
// in 0x80..0xff company; separators "\0", "\xff" and "é"; UTF-16
// parents (emoji, CJK) that take the generic path. Each loop both
// stores its parts (an owned copy) and reads them through the view.

function show(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c >= 0x20 && c < 0x7f) {
      out += s[i];
    } else {
      out += "\\x" + c.toString(16) + ";";
    }
  }
  return out;
}

function mk(codes: number[]): string {
  let s = "";
  for (const c of codes) s += String.fromCharCode(c);
  return s;
}

function fields(parts: string[], lens: number): string {
  let out = parts.length + "/" + lens + ":";
  for (const p of parts) out += "[" + show(p) + "]";
  return out;
}

function byComma(s: string): string {
  const parts: string[] = [];
  let lens = 0;
  for (const p of s.split(",")) {
    parts.push(p);
    lens += p.length;
  }
  return fields(parts, lens);
}

function byNul(s: string): string {
  const parts: string[] = [];
  let lens = 0;
  for (const p of s.split("\0")) {
    parts.push(p);
    lens += p.length;
  }
  return fields(parts, lens);
}

function byFF(s: string): string {
  const parts: string[] = [];
  let lens = 0;
  for (const p of s.split("\xff")) {
    parts.push(p);
    lens += p.length;
  }
  return fields(parts, lens);
}

function byEAcute(s: string): string {
  const parts: string[] = [];
  let lens = 0;
  for (const p of s.split("é")) {
    parts.push(p);
    lens += p.length;
  }
  return fields(parts, lens);
}

// view-only body: lengths and first code units, no part is kept
function lensComma(s: string): string {
  let out = "";
  for (const p of s.split(",")) {
    out += p.length + (p.length > 0 ? "@" + p.charCodeAt(0) : "") + " ";
  }
  return out;
}

function run(name: string, sep: number, split: (s: string) => string): void {
  // fillers: plain letters, the separator with its high bit or low bit
  // flipped, and 0x80 / 0xff / 0x00 when they are not the separator
  const fill: number[] = [0x61, sep ^ 0x80, sep ^ 0x01, 0x80, 0xff, 0x00, 0x62, 0x7f];
  const pool: number[] = [];
  for (const c of fill) if (c !== sep) pool.push(c);
  for (let len = 1; len <= 17; len++) {
    const base: number[] = [];
    for (let i = 0; i < len; i++) base.push(pool[(i + len) % pool.length]);
    console.log(name, len, "none", split(mk(base)));
    for (let pos = 0; pos < len; pos++) {
      const one = base.slice();
      one[pos] = sep;
      console.log(name, len, "at", pos, split(mk(one)));
    }
    // first and last byte, and a run of two in the middle
    const ends = base.slice();
    ends[0] = sep;
    ends[len - 1] = sep;
    if (len >= 4) {
      ends[len >> 1] = sep;
      ends[(len >> 1) + 1] = sep;
    }
    console.log(name, len, "ends", split(mk(ends)));
    const all: number[] = [];
    for (let i = 0; i < len; i++) all.push(sep);
    console.log(name, len, "all", split(mk(all)));
    // every other byte, phase 0 and phase 1
    for (let ph = 0; ph < 2; ph++) {
      const alt = base.slice();
      for (let i = ph; i < len; i += 2) alt[i] = sep;
      console.log(name, len, "alt", ph, split(mk(alt)));
    }
  }
}

run("comma", 0x2c, byComma);
run("nul", 0x00, byNul);
run("ff", 0xff, byFF);
run("eacute", 0xe9, byEAcute);

for (let len = 1; len <= 17; len++) {
  let s = "";
  for (let i = 0; i < len; i++) s += i % 3 === 2 ? "," : String.fromCharCode(0x41 + i);
  console.log("lens", len, lensComma(s));
}

console.log("empty", byComma(""), byNul(""), byFF(""), byEAcute(""));

// literal parents
console.log(byComma("a,b,c,d,e,f,g,h,i"));
console.log(byComma(",,,,,,,,"));
console.log(byComma("\xe9\xff,\x80\xac,\xad,"));
console.log(byEAcute("caf\xe9 au lait, cr\xe8me br\xfbl\xe9e"));
console.log(byFF("\xff\xfe\xff\x7f\xff"));
console.log(byNul("a\0b\0\0c\0"));

// UTF-16 parents: the separator is still one byte, the parent is not Latin-1
console.log(byComma("😀,a,中文,,🎉🎉,"));
console.log(byComma("中,文,字,符,串,分,割,测,试"));
console.log(byEAcute("é😀é中é"));
console.log(byFF("ÿ中ÿ\xffx"));
console.log(byNul("\0中\0"));
console.log(lensComma("😀,中文,,x"));
