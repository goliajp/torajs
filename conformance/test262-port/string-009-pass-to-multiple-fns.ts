// Adapted from test262: string passed to multiple readers.
function len(s: string): number { return s.length; }
function head(s: string): string { return s.slice(0, 3); }
function tail(s: string): string { return s.slice(3, s.length); }

function check(): number {
  let s: string = "hello";
  if (len(s) !== 5) { throw "#1"; }
  if (head(s) !== "hel") { throw "#2"; }
  if (tail(s) !== "lo") { throw "#3"; }
  if (len(s) + len(s) !== 10) { throw "#4"; }
  return 0;
}
console.log(check());
