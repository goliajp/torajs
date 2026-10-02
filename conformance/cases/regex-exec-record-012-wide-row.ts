// A row wider than the record (only `m[1]` of a five-group pattern is
// read), a record as wide as the row, optional and named groups among
// them, a surrogate-pair subject, and a lastIndex set past the end.

function narrow(s: string): string {
  const re = /(a)(b)?(c)(d)?(e)/g;
  let m = re.exec(s);
  let out = "";
  while (m !== null) {
    out = out + m.index + ":" + m[1] + "/" + m[0].length + ",";
    m = re.exec(s);
  }
  return out;
}
function wide(s: string): string {
  const re = /(a)(b)?(c)(d)?(e)/g;
  let m = re.exec(s);
  let out = "";
  while (m !== null) {
    out =
      out +
      m.index +
      ":" +
      m[0] +
      "|" +
      m[1] +
      "|" +
      (m[2] === undefined ? "-" : m[2]) +
      "|" +
      m[3] +
      "|" +
      (m[4] === undefined ? "-" : m[4].length) +
      "|" +
      m[5] +
      "|" +
      m[7] +
      ",";
    m = re.exec(s);
  }
  return out;
}
function named(s: string): string {
  const re = /(?<x>[a-z]+)=(?<y>\d+)(?<z>;)?/g;
  let m = re.exec(s);
  let out = "";
  while (m !== null) {
    out = out + m[1] + "=" + m[2].length + (m[3] === undefined ? "" : m[3]) + " ";
    m = re.exec(s);
  }
  return out + re.lastIndex;
}
function past(s: string): string {
  const re = /(\d)(\d)?/g;
  re.lastIndex = 50;
  let m = re.exec(s);
  let out = m === null ? "null " : "hit ";
  out = out + re.lastIndex + " ";
  m = re.exec(s);
  while (m !== null) {
    out = out + m.index + m[1] + (m[2] === undefined ? "_" : m[2]) + " ";
    m = re.exec(s);
  }
  return out + re.lastIndex;
}
console.log(narrow("ace abce acde abcde xace"));
console.log(wide("ace abce acde abcde xace"));
console.log(wide("😀ace 日本abcde"));
console.log(named("k=1;v=22 😀w=333;"));
console.log(past("a1b22c3"));
console.log(past("😀😀7"));
