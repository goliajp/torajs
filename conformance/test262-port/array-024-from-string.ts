// Adapted from test262: built-ins/Array/from/* — `Array.from(s)` over a
// string spreads it byte-by-byte into a `string[]`. tr's subset only
// covers the string-source overload (no iterator protocol, no mapFn);
// emitted as a Call to a runtime helper that walks the StrRepr and
// packs single-char strings into a fresh Array<Str>.
function check(): number {
  let xs = Array.from("abc");
  if (xs.length !== 3) { throw "#1: length"; }
  if (xs[0] !== "a") { throw "#2"; }
  if (xs[1] !== "b") { throw "#3"; }
  if (xs[2] !== "c") { throw "#4"; }

  let empty = Array.from("");
  if (empty.length !== 0) { throw "#5: empty"; }

  let one = Array.from("z");
  if (one.length !== 1) { throw "#6"; }
  if (one[0] !== "z") { throw "#7"; }

  // Roundtrip: split-by-char then rejoin.
  if (Array.from("hello").join("") !== "hello") { throw "#8: roundtrip"; }

  // Pipe through Array methods.
  let upper = Array.from("xy").map((c: string): string => c.toUpperCase()).join(",");
  if (upper !== "X,Y") { throw "#9: map+join"; }

  // Whitespace + punctuation preserved.
  let s = Array.from(" a,b ");
  if (s.length !== 5) { throw "#10: ws-len"; }
  if (s[0] !== " ") { throw "#11: leading ws"; }
  if (s[2] !== ",") { throw "#12: comma"; }

  return 0;
}
console.log(check());
