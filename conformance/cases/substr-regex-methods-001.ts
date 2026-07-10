// chunk 800 — a Substr view receiver (charAt / for-of char) fed to
// the regex-arg method family misread the view block as an owned
// Str header (match answered null, split answered mojibake); the
// dispatch station now materializes views through substr_to_owned.
const ch = "hello world".charAt(4);
console.log(ch.match(/o/));
console.log(ch.replace(/o/, "0"));
console.log(ch.split(/x/));
console.log([...ch.matchAll(/o/g)].length);
console.log(ch.search(/o/));
for (const c of "ab") {
  console.log(c.match(/a/));
  console.log(c.search(/b/));
}
