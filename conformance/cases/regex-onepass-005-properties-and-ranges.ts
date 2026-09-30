// A property class, a class range past U+00FF and a literal past
// U+00FF are tested against the code point: over a Latin-1 subject,
// over a UTF-16 one, and across a surrogate pair under `u`. Every
// probe prints the literal's answer and the `new RegExp` answer.

function probe(a: RegExp, s: string): boolean {
  const m = a.exec(s);
  if (m === null) {
    console.log("null", a.lastIndex);
    return false;
  }
  console.log(m.join("|"), m.index, m[0].length, a.lastIndex);
  return true;
}

function pair(a: RegExp, b: RegExp, s: string): void {
  probe(a, s);
  probe(b, s);
}

function walk(a: RegExp, s: string): void {
  while (probe(a, s)) {}
}

pair(/\p{L}+/u, new RegExp("\\p{L}+", "u"), "  Hello42 word  ");
pair(/\p{L}+/u, new RegExp("\\p{L}+", "u"), "12 Héllo÷x");
pair(/\p{L}+/u, new RegExp("\\p{L}+", "u"), "  Héllo日本 42");
pair(/\p{L}+/u, new RegExp("\\p{L}+", "u"), "12 𝒜𝒷c 3");
pair(/(\p{Lu})(\p{Ll}+)/u, new RegExp("(\\p{Lu})(\\p{Ll}+)", "u"), "x Ωμέγα Éa");
pair(/\P{L}+/u, new RegExp("\\P{L}+", "u"), "ab 12😀 cd");
pair(/[^\p{L}\d]+/u, new RegExp("[^\\p{L}\\d]+", "u"), "ab1 😀—cd");
pair(/\p{Script=Han}+/u, new RegExp("\\p{Script=Han}+", "u"), "kana かな 漢字 x");
pair(/[Ā-Ȁ]+/, new RegExp("[\\u0100-\\u0200]+"), "aĀāǿb");
pair(/[^一-鿿]+/u, new RegExp("[^\\u4e00-\\u9fff]+", "u"), "日本go😀語");
pair(/[ぁ-ん]+/, new RegExp("[ぁ-ん]+"), "漢字かなカナ");
pair(/(日+)本/, new RegExp("(日+)本"), "日日本");
pair(/(日|本)+語/u, new RegExp("(日|本)+語", "u"), "x日本日語");
pair(/😀+x/u, new RegExp("😀+x", "u"), "a😀😀x");
pair(/[😀-😏]+/u, new RegExp("[😀-😏]+", "u"), "a😀😄😐");
pair(/\p{L}+/u, new RegExp("\\p{L}+", "u"), "12 34");

walk(/(\p{L}+)=(\p{N}+)/gu, "k=1;日本=٣٤;é=5;=6");
walk(new RegExp("(\\p{L}+)=(\\p{N}+)", "gu"), "k=1;日本=٣٤;é=5;=6");
walk(/\p{L}+/gu, "ab cd");
walk(new RegExp("\\p{L}+", "gu"), "ab cd");
