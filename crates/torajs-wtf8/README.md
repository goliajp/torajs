# torajs-wtf8

WTF-8 ("Wobbly Transformation Format", Simon Sapin) — UTF-8 generalized so
that surrogate code points U+D800..U+DFFF get their natural 3-byte encoding.
Every sequence of UTF-16 code units — which is what an ECMAScript String
value is (§6.1.4) — therefore has exactly one well-formed WTF-8 spelling, and
a lone surrogate written as `"\uD800"` in source survives the trip from
lexer to `.rodata` byte-for-byte.

Well-formed WTF-8 never spells a surrogate *pair* as two 3-byte sequences:
`Wtf8Buf::push_code_point` and `push_wtf8` join a trailing high surrogate
with a leading low one into the 4-byte supplementary form, so byte equality
on the buffer is exactly code-unit equality on the JS string.

Valid UTF-8 is a subset: `Wtf8::as_str` hands back `&str` whenever the
buffer holds no lone surrogate, and `to_string_lossy` replaces each one
with U+FFFD for display.
