// Re-assignment from the same regex, truthiness and `!m` conditions,
// a ternary over the binding, and the group read as a string value.

function scan(s: string): string {
  const re = /(\w)(\d)?/g;
  let out = "";
  let m = re.exec(s);
  let n = 0;
  while (m) {
    n = n + 1;
    out = out + m[0] + ":" + m.index + ":" + m[0].length + ";";
    m = re.exec(s);
  }
  const last = re.exec("q");
  const tag = last ? "hit" : "miss";
  if (!last) {
    out = out + "!";
  }
  return out + n + tag;
}
console.log(scan("a1b2c"));
console.log(scan(""));
console.log(scan("x"));
