// A generator body may yield inside `for (v of s.split("sep"))`. The
// literal-separator loop walks the string through a cursor that does
// not survive a yield, and such a loop used to be rejected with "yield
// is only valid inside a `function*` generator body".
function* parts(s: string) {
  for (const p of s.split(",")) {
    yield p;
  }
  yield "end";
}
function* skipping(s: string) {
  for (const p of s.split(",")) {
    if (p === "b") continue;
    if (p === "d") break;
    yield p.toUpperCase();
  }
}
function* grid(s: string) {
  outer: for (const row of s.split(";")) {
    for (const cell of [row, "z"]) {
      if (cell === "x") continue outer;
      yield row + ":" + cell;
    }
  }
}
function* counted(s: string) {
  let n = 0;
  for (const p of s.split(" ")) {
    n += p.length;
  }
  yield n;
}
const log: string[] = [];
function* guarded(s: string) {
  try {
    for (const p of s.split("-")) {
      yield p;
      if (p === "2") return;
    }
  } finally {
    log.push("fin");
  }
}
console.log([...parts("a,b,c")].join("|"));
console.log([...skipping("a,b,c,d,e")].join("|"));
console.log([...grid("1,2;x;4")].join("|"));
console.log([...counted("ab cde f")].join("|"));
console.log([...guarded("1-2-3")].join("|"), log.join(","));
async function joined(s: string): Promise<string> {
  let out = "";
  for (const p of s.split("/")) {
    out += await Promise.resolve(p.length);
  }
  for (const p of s.split("/")) {
    out += p;
  }
  return out;
}
joined("ab/c/def").then((v) => console.log(v));
