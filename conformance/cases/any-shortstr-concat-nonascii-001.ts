// The Any-lane `+` fast path packs a concat result under six bytes
// into a NaN-boxed ShortStr immediate instead of allocating. That
// payload is UTF-8, while a heap Str payload is Latin-1 or UTF-16
// LE and its length counts code units — the two agree on ASCII and
// nowhere else, so every operand below has to reach `str_concat`.
// `var` bindings are what put these values in the Any lane.
var nbsp = String.fromCharCode(0xa0);
var pair = nbsp + nbsp;
console.log(pair.length, pair.charCodeAt(0), pair.charCodeAt(1));

var acute = "éx";
var tail = "y";
var joined = acute + tail;
console.log(JSON.stringify(joined), joined.length);

var wide = "中";
var widened = wide + "a";
console.log(JSON.stringify(widened), widened.length, widened.charCodeAt(0));

// ASCII of the same shape still takes the fast path and must be
// byte-identical to what it always answered.
var l = "ab";
var r = "cde";
console.log(JSON.stringify(l + r), (l + r).length);

// Leading Unicode whitespace through `+` then a numeric coercion —
// the shape that crashed the process.
var codes = [0x20, 0x09, 0xa0, 0x1680, 0x2000, 0x2028, 0x3000, 0xfeff];
for (var i = 0; i < codes.length; i++) {
  var ws = String.fromCharCode(codes[i]);
  console.log(codes[i], parseInt(ws + "1"), parseFloat(ws + "1.5"));
}
