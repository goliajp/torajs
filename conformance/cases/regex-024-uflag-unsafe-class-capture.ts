// chunk 10d v2 — u-flag unsafe class via UTF-8 byte expansion.
// These patterns hit the Pike VM second-pass capture recovery on
// byte_only Class leaves emitted by `utf8_class_expand`; previously
// these classes were Pike-VM-only (DFA disabled via the chunk 10c
// `prog_uses_uflag_unsafe_class` gate, removed in chunk 10d).
//
// Each capture-group case verifies that the DFA finds [start..end]
// and the second-pass Pike VM walks the same byte_only Class
// instructions to extract captures bytes-aligned with the cp.

// [^a]u — negate over a single ASCII cp.
{
  const m = "abc".match(/([^a]+)/u)
  console.log(m === null ? "null" : m[1])  // "bc"
}
{
  const m = "Ω漢字".match(/([^a]+)/u)
  console.log(m === null ? "null" : m[1])  // "Ω漢字"
}
{
  const m = "a😀b".match(/([^a])/u)
  console.log(m === null ? "null" : m[1])  // "😀"
}

// \p{L}u — UCD letter property with capture.
{
  const m = "  漢字 abc".match(/(\p{L}+)/u)
  console.log(m === null ? "null" : m[1])  // "漢字"
}
{
  const m = "x123Ω".match(/(\p{L})(\d+)(\p{L})/u)
  if (m === null) {
    console.log("null")
  } else {
    console.log(m[1] + "|" + m[2] + "|" + m[3])  // "x|123|Ω"
  }
}

// \P{L}u — negate property.
{
  const m = "abc 漢123".match(/(\P{L}+)/u)
  console.log(m === null ? "null" : m[1])  // " "
}

// [\p{L}\p{N}]u — union with capture.
{
  const m = "  a漢7  ".match(/([\p{L}\p{N}]+)/u)
  console.log(m === null ? "null" : m[1])  // "a漢7"
}

// [^\p{L}]u — class-level negate over property.
{
  const m = "ab 123!".match(/([^\p{L}]+)/u)
  console.log(m === null ? "null" : m[1])  // " 123!"
}

// Multi-capture combo: \p{L}+ then \P{L}+ then \p{L}+.
{
  const m = "abc 漢字".match(/(\p{L}+)(\P{L}+)(\p{L}+)/u)
  if (m === null) {
    console.log("null")
  } else {
    console.log(m[1] + "|" + m[2] + "|" + m[3])  // "abc| |漢字"
  }
}

// Replace with $1 referencing unsafe-class capture.
console.log("Ω abc 漢".replace(/(\p{L}+)/gu, "[$1]"))  // "[Ω] [abc] [漢]"

// .matchAll() with \P{L}+ — exercise iteration over DFA hits.
{
  const it = "a1b22c333d".matchAll(/(\P{L}+)/gu)
  const arr: string[] = []
  for (const m of it) {
    arr.push(m[1])
  }
  console.log(arr.join(","))  // "1,22,333"
}
