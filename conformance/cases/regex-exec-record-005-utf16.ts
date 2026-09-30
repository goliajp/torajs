// index and lengths are UTF-16 code units: a Latin-1 subject, a BMP
// subject and one with surrogate pairs ahead of the match.

function run(s: string): void {
  const re = /([a-z]+)=(\d+)/g;
  let m = re.exec(s);
  while (m !== null) {
    console.log(m.index, m[0].length, m[1].length, m[2].length, m[1], m[2], m.input === s);
    m = re.exec(s);
  }
  const re2 = /(\d+)/g;
  let n = re2.exec(s);
  while (n !== null) {
    console.log(n.index, n[1].length, n[1]);
    n = re2.exec(s);
  }
}
run("éa=1;éab=22");
run("日本a=3;x");
run("😀a=4;😀😀ab=55");
