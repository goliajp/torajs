// RFC 20260707-undefined-sentinel-repr chunk 1 — a missed exec/match
// capture slot (NULL Str per the 591 convention) must be safe through
// .length (catchable TypeError), the inline str-eq-with-literal fast
// path (false via the null-guarded str_eq), switch-on-string, and
// JSON.stringify (null inside an array). Nullable<Str> real-null
// print/eq behavior is unchanged.

const m = /a(b)?/.exec("a");
if (m !== null) {
  // inline eq fast path declines to the guarded runtime str_eq
  console.log(m[1] === "undefined");
  console.log(m[1] === "x");
  console.log(m[1] !== "y");
  // alias propagation
  const c = m[1];
  console.log(c === "undefined");
  const d = c;
  console.log(d === "z");
  // .length through the guard = catchable TypeError
  try {
    console.log(m[1].length);
  } catch (e) {
    console.log("caught-len", e instanceof TypeError);
  }
  try {
    console.log(c.length);
  } catch (e) {
    console.log("caught-alias-len", e instanceof TypeError);
  }
  // switch-on-string declines the inline byte walk
  switch (c) {
    case "b":
      console.log("case-b");
      break;
    default:
      console.log("case-default");
  }
  // JSON array lane: undefined slot stringifies to null
  console.log(JSON.stringify(m));
}

// hit path keeps working through every guarded shape
const h = /a(b)/.exec("ab");
if (h !== null) {
  console.log(h[1] === "b");
  console.log(h[1].length);
  const hc = h[1];
  switch (hc) {
    case "b":
      console.log("hit-case-b");
      break;
    default:
      console.log("hit-default");
  }
  console.log(JSON.stringify(h));
}

// real-null Nullable<Str> print/eq face unchanged (591-adjacent)
let s: string | null = null;
console.log(s);
console.log(s === null);
console.log("done");
