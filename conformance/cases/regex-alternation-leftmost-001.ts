// ES §22.2 alternation is leftmost-FIRST: a higher-priority branch that
// matches wins even when a lower-priority branch could match longer.
// tr's DFA is leftmost-longest, so dfa::analyze gates a prefix-
// overlapping alternation off the DFA onto the Pike VM (leftmost-first
// correct). Prefix-unrelated alternations keep the DFA fast path.
console.log(/1|12/.exec("123")![0]);
console.log(/a|ab/.exec("abc")![0]);
console.log(/foo|foobar/.exec("foobarbaz")![0]);
console.log("z11".match(/\d|\d\d/)![0]);
console.log("xyz".match(/x|xy|xyz/)![0]);
// prefix-unrelated: leftmost-first == leftmost-longest, DFA retained
console.log(/cat|dog/.exec("dogcat")![0]);
console.log(/POST|PUT/.exec("PUT")![0]);
// higher-priority branch already longer: no disagreement either way
console.log("a1b2".match(/[a-z]\d|[a-z]/)![0]);
