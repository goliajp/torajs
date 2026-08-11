// RFC 20260812-console-sink knife 3 — console.error / console.warn as
// VALUES: ns-static cells (append-only rows behind the URI quartet)
// whose dispatch arm brackets the shared inline-print walk with the
// io current-sink switch. Covers the cell call (detached alias), the
// identity face, name/length, and the dynamic lane; stderr bytes are
// compared to bun manually in pre-flight (the oracle only diffs
// stdout). The dynamic lane (`(console as any).error`) rides the
// knife-4 singleton fixture — bare `console` in a value position
// is that knife's face.
const e = console.error;
const w = console.warn;
e("cell stderr", 1);
w({ a: [1, 2] });
console.log(e === console.error);
console.log(e.name, e.length);
console.log(w.name, w.length);
console.log("stdout-after");
