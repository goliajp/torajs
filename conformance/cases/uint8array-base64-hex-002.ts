// §23.2.2.1-2 — the two `Uint8Array` statics. Unlike `setFrom*` there
// is no buffer to run out of, so the whole answer is a new array or a
// throw. Vectors are RFC 4648's.
for (const text of ["", "Zg==", "Zm8=", "Zm9v", "Zm9vYg==", "Zm9vYmE=", "Zm9vYmFy"]) {
  const a = Uint8Array.fromBase64(text);
  console.log(text || "(empty)", a.length, a.buffer.byteLength, a.join(","));
}
console.log(Uint8Array.fromHex("666f6f626172").join(","));
console.log(Uint8Array.fromHex("666F6f").join(","));
console.log(Uint8Array.fromHex("").length);

// The result really is a Uint8Array, not something array-shaped.
const one = Uint8Array.fromBase64("Zm9v");
console.log(Object.getPrototypeOf(one) === Uint8Array.prototype, one instanceof Uint8Array);

// Alphabets do not overlap, whitespace is skipped, and the last-chunk
// modes disagree exactly where §23.2 says they do.
console.log(Uint8Array.fromBase64("x+/y").join(","));
console.log(Uint8Array.fromBase64("x-_y", { alphabet: "base64url" }).join(","));
console.log(Uint8Array.fromBase64("Z g==").join(","));
console.log(Uint8Array.fromBase64("ZXhhZg", { lastChunkHandling: "loose" }).join(","));
console.log(Uint8Array.fromBase64("ZXhhZg", { lastChunkHandling: "stop-before-partial" }).join(","));
console.log(Uint8Array.fromBase64("ZXhhZh==", { lastChunkHandling: "loose" }).join(","));

function throws(f: () => void): string {
  try { f(); return "no throw"; } catch (e: any) { return e.constructor.name; }
}
console.log(throws(() => { Uint8Array.fromBase64("x+/y", { alphabet: "base64url" }); }));
console.log(throws(() => { Uint8Array.fromBase64("ZXhhZg", { lastChunkHandling: "strict" }); }));
console.log(throws(() => { Uint8Array.fromBase64("ZXhhZh==", { lastChunkHandling: "strict" }); }));
console.log(throws(() => { Uint8Array.fromBase64("Zm.9v"); }));
console.log(throws(() => { Uint8Array.fromBase64("Zg =="); }));
console.log(throws(() => { Uint8Array.fromBase64(5 as any); }));
console.log(throws(() => { Uint8Array.fromBase64("Zg==", "nope" as any); }));
console.log(throws(() => { Uint8Array.fromHex("a"); }));
console.log(throws(() => { Uint8Array.fromHex("zz"); }));
console.log(throws(() => { Uint8Array.fromHex(5 as any); }));
