// ES 25.5.2.2 QuoteJSONString step 2.d - a surrogate code unit that
// is not half of a well-formed pair has no UTF-8 spelling, so a
// well-formed JSON text carries it as a \uXXXX escape. A paired
// half passes through as the code point it is half of.
const emoji = "\u{1F600}";
const lead = emoji.slice(0, 1);
const trail = emoji.slice(1);

console.log(JSON.stringify(emoji));
console.log(JSON.stringify(lead));
console.log(JSON.stringify(trail));
console.log(JSON.stringify(lead + trail));
console.log(JSON.stringify(trail + lead));
console.log(JSON.stringify([lead, "a", trail]));
console.log(JSON.stringify({ k: lead }));
console.log(JSON.stringify("a" + lead + "b"));
// NOT covered here: a lone surrogate written as an escape in a
// SOURCE literal. String literals live in the AST as Rust `String`,
// which is UTF-8 and cannot hold one, so `"\ud800"` is already
// U+FFFD by the time any of this runs. Carrying it needs a
// code-unit (WTF-8) representation for literals, which is a change
// to the lexer, the AST and the literal baking together.

// Escapes and control characters keep their own shapes beside it.
console.log(JSON.stringify(lead + String.fromCharCode(34) + String.fromCharCode(10)));

// The any lane answers the same text.
const asAny: any = [lead, emoji];
console.log(JSON.stringify(asAny));

// Round trip: the escape parses back to the same lone surrogate.
console.log(JSON.parse(JSON.stringify(lead)).charCodeAt(0));
console.log(JSON.parse(JSON.stringify(emoji)) === emoji);
