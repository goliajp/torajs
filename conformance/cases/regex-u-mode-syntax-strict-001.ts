// §22.2.1 Pattern[+UnicodeMode] — `{`, `}`, `]` are SyntaxCharacters
// and a SyntaxCharacter is not a PatternCharacter; the annexB
// ExtendedPatternCharacter lenience that reads them as literals
// exists only OUTSIDE Unicode mode. A malformed brace body (`x{`,
// `x{,3}`), a stray `}`, or a stray `]` must be a SyntaxError under
// `u` (and `v`, its superset) while staying accepted without the
// flag. Well-formed quantifiers keep working in every mode.
function t(p: string, f: string) {
  try {
    new RegExp(p, f);
    console.log("ok", JSON.stringify(p), f);
  } catch (e: any) {
    console.log("err", JSON.stringify(p), f, e instanceof SyntaxError);
  }
}
t("x{", "u");
t("x{", "");
t("}", "u");
t("}", "");
t("]", "u");
t("]", "");
t("x{2}", "u");
t("x{,3}", "u");
t("x{,3}", "");
t("{", "v");
t("x{2,3}", "v");
