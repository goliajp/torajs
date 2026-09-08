--> a close-comment on the very first line
// annexB §B.1.3 — HTML-like comments are Script-only, which is why
// this fixture is a `.cts`. `-->` opens one when nothing but
// whitespace or delimited comments precedes it on its line, and the
// first line qualifies with no LineTerminator in front of it at all
// (`InputElementHashbangOrRegExp`). If that line were not a comment
// this file would not parse, so every line below is the test.
console.log("first line");

   --> leading whitespace still qualifies
console.log("leading whitespace");

/* one */ /*two*/--> delimited comments may precede the marker
console.log("after delimited comments");

var afterCode = 4;
--> and the ordinary form, on its own line after real code
console.log(afterCode);
