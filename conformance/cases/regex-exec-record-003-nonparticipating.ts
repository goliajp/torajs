// A group that did not participate reads `undefined` through the
// record, and `.length` on it is the TypeError `undefined.length`
// raises; a caught throw leaves the loop's next exec intact.

function run(s: string): void {
  const re = /a(b)?(c)?/g;
  let m = re.exec(s);
  while (m !== null) {
    console.log(m[0], m[1], m[2], typeof m[2], m[2] === undefined, m[2] == null);
    try {
      console.log(m[2].length);
    } catch (e) {
      console.log("caught", e instanceof TypeError);
    }
    console.log(m[1] === undefined ? -1 : m[1].length);
    m = re.exec(s);
  }
}
run("abc ab a ac");
