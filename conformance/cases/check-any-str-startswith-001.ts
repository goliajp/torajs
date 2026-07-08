// Chunk 692 — String.prototype.startsWith / endsWith through an
// `any` receiver (any-method-call RFC 20260704 C3+ arm): the
// Tag::Str arm ToStrings the needle and wraps the typed tier's
// encoding-aware prefix / suffix kernels (clamping and the
// empty-needle always-match live in the kernel).
const m: any = "undefined is not an object";
console.log(m.startsWith("undefined"));
console.log(m.startsWith("null"));
console.log(m.endsWith("object"));
console.log(m.endsWith("object", 10));
console.log(m.startsWith("is", 10));
console.log(m.startsWith(""));
console.log(m.endsWith(""));
// the recorded motivating shape: a caught error's `.message` is
// `any`, and `.startsWith` runs on it directly (no string cast)
try {
  const u: any = undefined;
  Object.getOwnPropertyDescriptor(u, "x");
} catch (e: any) {
  console.log(e.message.startsWith("undefined"));
}
