// The byte walk consumes a run of bytes that all transition back into
// the same state without re-reading the state row. The run has to end
// on exactly the byte that leaves the state, and nothing inside it may
// move the leftmost-longest accept — so every probe here is a place
// where getting the boundary wrong would show.
//
// `a.+c` is the shape: after `a` the `.+` state self-loops on every
// byte but `c`, and it is not an accepting state, so the older
// accept-gated run never covered it.
const probes: string[] = [
  // greedy: the LAST `c` wins, so the run has to be re-entered after
  // each `c` rather than swallowing it
  JSON.stringify("pre a middle c post".match(/a.+c/)),
  JSON.stringify("a-c-c-c-end".match(/a.+c/)),
  JSON.stringify("acacac".match(/a.+c/)),
  // the `s` flag lets the run cross newlines; without it `.` stops
  JSON.stringify("pre a\nmiddle\nc post".match(/a.+c/s)),
  JSON.stringify("pre a\nmiddle\nc post".match(/a.+c/)),
  JSON.stringify("a\nc".match(/a.+c/s)),
  // run of length 1, and a run that reaches the end without accepting
  JSON.stringify("axc".match(/a.+c/)),
  JSON.stringify("axxxxxxxxxxxx".match(/a.+c/)),
  JSON.stringify("ac".match(/a.+c/)),
  // a zero-width accept sitting on a self-looping state: the run must
  // not skip past the boundary `\b` resolves at
  JSON.stringify("say hello world now".match(/\bhello\b.*\bnow\b/)),
  JSON.stringify("aaa bbb".match(/\ba+\b/)),
  JSON.stringify("xx aaa xx".match(/\ba+\b/)),
  JSON.stringify("aaab".match(/\ba+\b/)),
  // class self-loops, including one whose exit is the accepting state
  JSON.stringify("contact me at bob@example now".match(/[a-z]+@[a-z]+/)),
  JSON.stringify("   hello   ".match(/[a-z]+/)),
  JSON.stringify("0123456789x".match(/[0-9]+/)),
  // multi-byte characters inside the run
  JSON.stringify("a日本語c".match(/a.+c/)),
  JSON.stringify("a日本語c".match(/a.+c/u)),
  JSON.stringify("  日本語  ".match(/\p{L}+/u)),
  // the run reaching end-of-input with an at-end accept pending
  JSON.stringify("aaaa".match(/a+$/)),
  JSON.stringify("xaaaa".match(/a+$/)),
];
for (const p of probes) console.log(p);
