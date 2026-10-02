// RegExp v flag — class strings (\q{…}) lowered as a shared-prefix
// trie. The spec tries the strings of a class in descending length
// (CompileAtom, Atom :: CharacterClass), then the single code points,
// then the empty string; the trie reaches the deepest match first and
// backs off to shorter prefixes. Exercised on the DFA, the Pike VM
// (captures, backreferences) and the reverse VM (lookbehind).

// longest prefix wins, whatever the \q order
console.log("abc".match(/[\q{abc|ab|a}]/v)![0], "abc".match(/[\q{a|ab|abc}]/v)![0]);
console.log("ab".match(/[\q{abc|ab|a}]/v)![0], "a".match(/[\q{abc|ab|a}]/v)![0]);

// the continuation backs the class off to the shorter string
console.log(/^[\q{abc|ab}]c$/v.test("abc"), /^[\q{abc|ab}]$/v.test("abc"));
{
  const m = /([\q{abc|ab}])(c?)/v.exec("abc")!;
  console.log(m[0], m[1], JSON.stringify(m[2]));
}

// backreference forces the Pike VM; the capture is the string the
// class finally settled on
{
  const m = /([\q{abc|ab}])\1/v.exec("ababc")!;
  console.log(m[0], m[1]);
}
{
  const m = /([\q{abc|ab}])\1/v.exec("abcabc")!;
  console.log(m[0], m[1]);
}

// lookbehind walks the strings backwards; length order still holds
console.log(/(?<=[\q{ab|b}])c/v.exec("abc")!.index, /(?<=[\q{xab|b}])c/v.exec("xabc")!.index);
{
  const m = /(?<=([\q{ab|b}]))c/v.exec("abc")!;
  console.log(m[1]);
}

// strings sharing prefixes fan out once per differing position
console.log(
  "cat cab car cow".match(/[\q{cat|cab|car|cow|co}]/gv)!.join(","),
  /^[\q{cat|cab|car|cow|co}]+$/v.test("cocatcow"),
  /^[\q{cat|cab|car|cow|co}]+$/v.test("cod")
);

// single code points and the empty string come after the strings
console.log("ab".match(/[\q{ab}a]/v)![0], /^[\q{ab|}]$/v.test(""), /^[\q{ab|}]x$/v.test("abx"));

// quantified class re-picks per iteration
console.log(/^[\q{ab|c}]+$/v.test("abcab"), "abcabx".match(/[\q{ab|c}]+/v)![0]);

// ignoreCase applies to the string members
console.log(/[\q{AB|cd}]/vi.test("ab"), /[\q{AB|cd}]/vi.test("CD"), /[\q{AB}]/v.test("ab"));

// non-ASCII strings share their leading bytes
console.log("éa éb".match(/[\q{éa|éb}]/gv)!.join(","), /^[\q{日本|日}]+$/v.test("日本日"));
console.log("日本語".match(/[\q{日本|日本語}]/v)![0], "日本".match(/[\q{日本|日本語}]/v)![0]);

// replace / split through the same class
console.log("abcab-ab-abc".replace(/[\q{abc|ab}]/gv, "<$&>"), "xabyabcz".split(/[\q{abc|ab}]/v).join("|"));
