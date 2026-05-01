// Adapted from test262: built-ins/{String,Array}/prototype/at/* +
// built-ins/String/fromCharCode/* — negative-index access plus the
// inverse of charCodeAt.
//
// Subset constraint: out-of-bounds `.at(i)` is UB (no Nullable<T> for
// non-pointer T; tests keep `i` in [-len, len-1]). The bun-portable
// shape covers the common-case in-bounds paths only.
function check(): number {
  // String.at — positive + negative.
  if ("hello".at(0) !== "h") { throw "#1"; }
  if ("hello".at(4) !== "o") { throw "#2"; }
  if ("hello".at(-1) !== "o") { throw "#3: -1 wraps to last"; }
  if ("hello".at(-5) !== "h") { throw "#4: -len wraps to first"; }
  if ("hello".at(2) !== "l") { throw "#5"; }
  if ("hello".at(-3) !== "l") { throw "#6"; }

  // Array.at — positive + negative.
  let xs: number[] = [10, 20, 30, 40, 50];
  if (xs.at(0) !== 10) { throw "#7"; }
  if (xs.at(2) !== 30) { throw "#8"; }
  if (xs.at(-1) !== 50) { throw "#9"; }
  if (xs.at(-3) !== 30) { throw "#10"; }
  if (xs.at(4) !== 50) { throw "#11"; }

  // String[] at.
  let names: string[] = ["alpha", "beta", "gamma"];
  if (names.at(-1) !== "gamma") { throw "#12"; }
  if (names.at(0) !== "alpha") { throw "#13"; }

  // String.fromCharCode — ASCII roundtrip with charCodeAt.
  if (String.fromCharCode(65) !== "A") { throw "#14"; }
  if (String.fromCharCode(97) !== "a") { throw "#15"; }
  if (String.fromCharCode(48) !== "0") { throw "#16"; }

  let alpha = "alphabet";
  let first_code = alpha.charCodeAt(0);
  if (String.fromCharCode(first_code) !== "a") { throw "#17: roundtrip"; }
  return 0;
}
console.log(check());
