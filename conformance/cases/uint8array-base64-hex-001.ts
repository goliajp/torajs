// §23.2.3's four `Uint8Array.prototype` text conversions. The vectors
// are RFC 4648's, which is where test262 takes them from too.
const enc = new Uint8Array([102, 111, 111, 98, 97, 114]);
console.log(enc.toBase64(), enc.toHex());
console.log(new Uint8Array([]).toBase64(), new Uint8Array([]).toHex());
console.log(new Uint8Array([102]).toBase64(), new Uint8Array([102, 111]).toBase64());

// The two options `toBase64` reads, and `omitPadding` through
// ToBoolean rather than a type check.
console.log(
  new Uint8Array([199, 239]).toBase64({ alphabet: "base64url" }),
  new Uint8Array([199, 239]).toBase64({ omitPadding: true }),
  new Uint8Array([255]).toBase64({ alphabet: "base64url", omitPadding: true }),
  new Uint8Array([255]).toBase64({ omitPadding: 0 as any }),
);

// A chunk lands whole or not at all, so a buffer that cannot hold the
// next chunk stops before it — and `read` reports the index of the
// last one that did land.
function set(size: number, text: string, opts?: any): string {
  const t = new Uint8Array(size);
  const r: any = opts === undefined ? t.setFromBase64(text) : t.setFromBase64(text, opts);
  return r.read + "/" + r.written + " [" + t.join(",") + "]";
}
console.log(set(6, "Zm9vYmFy"));
console.log(set(5, "Zm9vYmFy"));
console.log(set(5, "Zm9vYmE="));
console.log(set(4, "Zm9vYmE="));
console.log(set(5, "Zm9vYmE"));
console.log(set(5, "Zm9vYmE", { lastChunkHandling: "stop-before-partial" }));

// ASCII whitespace is skipped; anything else outside the alphabet is
// a SyntaxError, and the alphabets do not overlap.
const ws = new Uint8Array(1);
ws.setFromBase64("Z g==");
console.log(ws[0]);
const url = new Uint8Array(3);
url.setFromBase64("x-_y", { alphabet: "base64url" });
console.log(url.join(","));

function throws(f: () => void): string {
  try { f(); return "no throw"; } catch (e: any) { return e.constructor.name; }
}
console.log(throws(() => { new Uint8Array(4).setFromBase64("x-_y"); }));
console.log(throws(() => { new Uint8Array(4).setFromBase64("Zg =="); }));
console.log(throws(() => { new Uint8Array(4).setFromBase64("ZXhhZg", { lastChunkHandling: "strict" }); }));
console.log(throws(() => { new Uint8Array(4).setFromBase64("ZXhhZg==="); }));
console.log(throws(() => { new Uint8Array(4).toBase64({ alphabet: "nope" }); }));
console.log(throws(() => { new Uint8Array(4).setFromHex(5 as any); }));
console.log(throws(() => { (Uint8Array.prototype as any).toHex.call([]); }));

// The decoded prefix is written, and only then does the error rise.
const partial = new Uint8Array([255, 255, 255, 255, 255]);
console.log(throws(() => { partial.setFromBase64("MjYyZm.9v"); }), partial.join(","));

// Hex takes no options. An odd length answers before the decode
// loop is reached, so nothing lands; an illegal character keeps the
// pairs before it. Both cases of the digits decode.
const hx = new Uint8Array(4);
const hr: any = hx.setFromHex("666F6f62");
console.log(hr.read + "/" + hr.written, hx.join(","));
const odd = new Uint8Array(4);
console.log(throws(() => { odd.setFromHex("aabbc"); }), odd.join(","));
const bad = new Uint8Array(4);
console.log(throws(() => { bad.setFromHex("aabbcz"); }), bad.join(","));
const small = new Uint8Array(2);
const sr: any = small.setFromHex("aabbcc");
console.log(sr.read + "/" + sr.written, small.join(","));

// These four are Uint8Array's, not every typed array's.
const other = new Int8Array(2);
console.log("toHex" in enc, "toHex" in other, typeof (other as any).toBase64);
console.log(typeof (enc as any).setFromHex, (enc as any).toHex.name, (enc as any).setFromBase64.length);
