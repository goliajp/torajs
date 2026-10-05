// §25.5.1: a JSON string may not hold an unescaped code unit below
// U+0020, an escape outside the JSON set, or a \u with non-hex digits,
// and every text that does not parse is a SyntaxError
const cases = [
  '"\u0000\u0001"',
  '"ab' + "c'",
  '"a\tb"',
  '"a\nb"',
  '"\u001f"',
  '"ok"',
  '"\u007f"',
  '"\\x41"',
  '"\\u00zz"',
  '"\\u0041\\n\\/\\b"',
  '"\\a"',
  "[1, 2",
  '{"a": "\u0002"}',
];
for (const s of cases) {
  try {
    const v = JSON.parse(s);
    console.log("ok", JSON.stringify(v));
  } catch (e: any) {
    console.log("err", e instanceof SyntaxError);
  }
}
