// Chunk 657 — JSON.stringify of UTF-16-encoded strings. The quote
// helpers read `length` as a byte count over the raw payload, so a
// UTF-16 block ("中", stored LE as 2d 4e) leaked its encoding bytes
// into the output ("-N"). The quote path now stays in-encoding
// (UTF-16 in → UTF-16 out); the jsb builder decodes to UTF-8 and
// re-classifies on finalize.

// 1) Top-level CJK string.
console.log(JSON.stringify("中文!"));

// 2) exec-result array (the u1 probe shape) — lookahead capture,
// plain capture, lookbehind over ASCII and CJK.
console.log(JSON.stringify(/(?=(\p{L}+))./u.exec("中文!")));
console.log(JSON.stringify(/(\p{L}+)/u.exec("中文!")));
console.log(JSON.stringify(/(?<=(\p{L}+))!/u.exec("ab!")));
console.log(JSON.stringify(/(?<=(\p{L}+))!/u.exec("中文!")));

// 3) Struct fast path (jsb builder) — CJK value among primitives.
const o = { id: 7, name: "李雷", ok: true };
console.log(JSON.stringify(o));

// 4) Escapes still fire inside a UTF-16 string.
console.log(JSON.stringify('中"文\n'));

// 5) Latin-1 supplement regression (é stays Latin-1-encoded).
console.log(JSON.stringify("café"));

// 6) Surrogate pair (astral plane).
console.log(JSON.stringify("a😀b"));
