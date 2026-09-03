// §12.9.5 RegularExpressionLiteral + §22.2.1 RegExpIdentifierName
// (rotation 576). The refusals — a line terminator inside the body, a
// unicode escape in the flags, a dangling `\k<name>`, a name that
// starts with a digit — are negative cases and live in test262. This
// fixture is the other side: the shapes near those gates that are
// LEGAL and must keep working.
//
// A group name is an identifier, not a run of `[A-Za-z0-9_]`. tr
// refused every spelling below but the plain-ASCII one before this
// rotation.
function nameOf(re: RegExp, s: string, key: string): string {
  const m = re.exec(s);
  if (m === null) return "no match";
  return String((m as any).groups[key]);
}
console.log(nameOf(/(?<$x>a)/, "a", "$x"));
console.log(nameOf(/(?<_x>a)/, "a", "_x"));
console.log(nameOf(/(?<x$9>ab)/, "ab", "x$9"));
console.log(nameOf(/(?<foo>a)/, "a", "foo"));
console.log(nameOf(/(?<日>a)/, "a", "日"));
// A supplementary character is one identifier character, spelled
// either as a braced escape or as its surrogate pair; both name the
// same group, and a reference may pick the other spelling.
console.log(/(?<\u{1d453}>a)\k<𝑓>/.test("aa"));
console.log(nameOf(/(?<𝑓>ab)/, "ab", "\u{1d453}"));
// The name carries into the replacement's `$<name>` form.
console.log("a".replace(/(?<$a>a)/, "[$<$a>]"));
// The body: `/` is bare inside a class and escaped outside; a
// backslash before any other character still escapes it.
console.log(/[/]/.test("/"), /a\/b/.test("a/b"), /a\.b/.test("a.b"));
console.log(/\\/.test("\\"), /a\tb/.test("a\tb"));
// Flags are an IdentifierPart run, so every valid letter still
// attaches — and only the letters are valid ones.
console.log(/a/gimsy.flags, /A/iu.test("a"), /a/.flags === "");
// Annex B keeps its two lenient readings outside `u`: a `\k` in a
// pattern with no named group is a literal `k`, and a decimal escape
// past the capture count rereads as a legacy octal.
console.log(/\k<a>/.test("k<a>"), /\101/.test("A"));
