// RFC 20260707 residual — top-level `JSON.stringify(undefined)`
// answers the undefined VALUE (ES §25.5.1 step 12), not the string
// "null"; inside an array/object undefined still stringifies to
// `null` (§25.5.2.4). The Str/Substr top-level lanes route the
// undefined sentinel through `__torajs_json_quote_str_top`.

const m = /a(b)?/.exec("a");
if (m !== null) {
  // 1) Top-level missed capture (undefined) — prints "undefined".
  console.log(JSON.stringify(m[1]));
  // 2) Top-level hit capture — quoted string.
  console.log(JSON.stringify(m[0]));
  // 3) Composite lane unchanged: undefined element → null.
  const xs = ["b", m[1], "a"];
  console.log(JSON.stringify(xs));
}

// 4) Top-level real string — quote helper delegation.
console.log(JSON.stringify("x"));

// 5) Top-level null stays the string "null" (§25.5.2).
console.log(JSON.stringify(null));
