// rotation 470 — a replacement with no `$` now appends whole instead
// of walking byte by byte. The two branches have to keep answering the
// same things, so both are exercised here side by side.

// literal replacements: the fast path
console.log("abbc xxx abc yyy abbbbc".replace(/a(b+)c/g, "XY"));
console.log("foo bar foo".replace("foo", "baz"));
console.log("aaa".replace(/a/g, ""));
console.log("aaa".replace(/a/g, "--"));
console.log("hello".replace(/l/, "L"));
console.log("x".replace(/(?:)/g, "-"));
console.log("日本語".replace(/本/, "ホン"));

// `$` forms: the expansion path, all of it
console.log("abbc".replace(/a(b+)c/, "[$1]"));
console.log("abbc".replace(/a(b+)c/, "[$&]"));
console.log("xxabbcyy".replace(/a(b+)c/, "[$`|$']"));
console.log("abbc".replace(/a(b+)c/, "100$$"));
console.log("abbc".replace(/a(?<mid>b+)c/, "[$<mid>]"));
console.log("abc abc".replace(/(a)(b)(c)/g, "$3$2$1"));
console.log("abc".replace(/(a)(b)(c)/, "$0$4$99"));

// a `$` that is not a valid form stays literal
console.log("abc".replace(/b/, "$"));
console.log("abc".replace(/b/, "$z"));
console.log("abc".replace(/b/, "a$"));

// unmatched group in an expansion is the empty string
console.log("ac".replace(/a(x)?c/, "[$1]"));

// replaceAll takes the same path
console.log("a-a-a".replaceAll("a", "b"));
console.log("a1a2a".replaceAll(/a/g, "$&$&"));

// function replacements bypass both branches
console.log("abbc".replace(/a(b+)c/, (m: string, g1: string) => g1.length.toString()));
