// Adapted from test262: built-ins/String/prototype/{replace,replaceAll}/* —
// literal-needle replace and replaceAll. JS allows a regex needle; v0
// only supports the string-needle form (no regex layer yet). The
// runtime helper does a manual memcmp scan and a single-malloc copy.
function check(): number {
  // replace — first occurrence only.
  if ("hello world".replace("world", "earth") !== "hello earth") { throw "#1"; }
  if ("aaaa".replace("a", "b") !== "baaa") { throw "#2: only first"; }
  if ("foobar".replace("zzz", "x") !== "foobar") { throw "#3: not found, returns copy"; }
  if ("abc".replace("", "x") !== "xabc") { throw "#4: empty needle prepends"; }

  // replace — needle = entire string.
  if ("hello".replace("hello", "world") !== "world") { throw "#5"; }

  // replace — replacement longer / shorter than needle.
  if ("ab".replace("a", "xyz") !== "xyzb") { throw "#6: longer repl"; }
  if ("abc".replace("abc", "x") !== "x") { throw "#7: shorter repl"; }

  // replaceAll — every occurrence.
  if ("foo-bar-baz".replaceAll("-", "_") !== "foo_bar_baz") { throw "#8"; }
  if ("aaaa".replaceAll("a", "X") !== "XXXX") { throw "#9"; }
  if ("ababab".replaceAll("ab", "C") !== "CCC") { throw "#10"; }
  if ("aaaa".replaceAll("aa", "X") !== "XX") { throw "#11: non-overlap"; }
  if ("abc".replaceAll("z", "x") !== "abc") { throw "#12: not found"; }

  // replaceAll — replacement larger than needle.
  if ("a-b-c".replaceAll("-", "++") !== "a++b++c") { throw "#13"; }
  // replaceAll — replacement shorter than needle.
  if ("xabxabxab".replaceAll("xab", "y") !== "yyy") { throw "#14: shorter repl"; }

  // Edge — empty source.
  if ("".replace("a", "b") !== "") { throw "#15"; }
  if ("".replaceAll("a", "b") !== "") { throw "#16"; }
  return 0;
}
console.log(check());
