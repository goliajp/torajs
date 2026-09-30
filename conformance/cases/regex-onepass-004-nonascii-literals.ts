// A pattern character past ASCII is one character, whatever engine
// runs the pattern: a quantifier after it repeats the whole character,
// an escape names the same character as its literal spelling, and a
// supplementary character is one step under `u` and two code units
// without it. Every probe prints the literal's answer and the
// `new RegExp` answer over Latin-1 and UTF-16 subjects.

function pair(a: RegExp, b: RegExp, s: string): void {
  console.log(a.exec(s), a.lastIndex);
  console.log(b.exec(s), b.lastIndex);
}

// a lone surrogate prints differently across runtimes; its span does not
function span(a: RegExp, b: RegExp, s: string): void {
  const m = a.exec(s);
  const n = b.exec(s);
  console.log(m === null ? -1 : m.index, m === null ? -1 : m[0].length);
  console.log(n === null ? -1 : n.index, n === null ? -1 : n[0].length);
}

function walk(a: RegExp, s: string): void {
  let m = a.exec(s);
  while (m !== null) {
    console.log(m, a.lastIndex);
    m = a.exec(s);
  }
  console.log("end", a.lastIndex);
}

pair(/(é+)=/, new RegExp("(é+)="), "éé=22");
pair(/(é+)=/, new RegExp("(é+)="), "xéé=22 日本");
pair(/é*z/, new RegExp("é*z"), "ééz");
pair(/é?a/, new RegExp("é?a"), "éa a");
pair(/é{2}/, new RegExp("é{2}"), "aééé");
pair(/(日+)本/, new RegExp("(日+)本"), "日日本");
pair(/(日|本)+語/, new RegExp("(日|本)+語"), "x日本日語");
pair(/\xe9+/, new RegExp("\\xe9+"), "aééb");
pair(/\u00e9+/, new RegExp("\\u00e9+"), "aééb");
pair(/\u65e5+/, new RegExp("\\u65e5+"), "a日日b");
pair(/[é]+/, new RegExp("[é]+"), "aééb");
pair(/😀+/u, new RegExp("😀+", "u"), "a😀😀b");
pair(/😀+/, new RegExp("😀+"), "a😀😀b");
pair(/\u{1F600}+/u, new RegExp("\\u{1F600}+", "u"), "a😀😀b");
pair(/\uD83D\uDE00+/u, new RegExp("\\uD83D\\uDE00+", "u"), "a😀😀b");
span(/\uD83D\uDE00+/, new RegExp("\\uD83D\\uDE00+"), "a😀😀\uDE00b");
span(/\uD83D/u, new RegExp("\\uD83D", "u"), "😀\uD83Dx");
span(/\uD83D/, new RegExp("\\uD83D"), "😀\uD83Dx");
pair(/(?<=é)a/, new RegExp("(?<=é)a"), "aéa");

walk(/(é+)=(\d+)/g, "éé=22;é=3;=4");
walk(new RegExp("(é+)=(\\d+)", "g"), "éé=22;é=3;=4");
walk(/(é+)=(\d+)/g, "éé=22;日é=3;=4");
walk(new RegExp("(é+)=(\\d+)", "g"), "éé=22;日é=3;=4");
