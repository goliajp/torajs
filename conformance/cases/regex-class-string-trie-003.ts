// RegExp v flag — a class string followed by an optional group. The
// spec tries the class's strings before its single code points, so
// `/[\q{ab}a](?:bc)?/v.exec("abc")[0]` is "ab" (the group matches
// nothing). tr answers "abc" here: the literal is DFA-eligible and the
// DFA lane takes the overall-longest match, which predates the trie
// lowering (the alternation form was eligible too). The anchored
// forms below agree with the spec on both engines.

console.log(/^[\q{ab}a](?:bc)?$/v.test("abc"), /^[\q{ab}a](?:bc)?$/v.test("ab"));
console.log(/^[\q{ab}a](?:bc)?$/v.test("a"), /^[\q{ab}a](?:bc)?$/v.test("abcbc"));
