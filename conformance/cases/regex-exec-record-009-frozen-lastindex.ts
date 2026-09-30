// A read-only lastIndex makes a global exec throw TypeError at the
// lastIndex store (§22.2.7.2); the throw is catchable and the binding
// is left as it was.

function run(): void {
  const re = /(\d)/g;
  let m = re.exec("a1b2");
  console.log(m !== null, m !== null ? m.index : -1);
  Object.defineProperty(re, "lastIndex", { writable: false });
  try {
    m = re.exec("a1b2");
    console.log("no throw", m === null);
  } catch (e) {
    console.log("caught", e instanceof TypeError);
  }
  const plain = /(\d)/;
  let p = plain.exec("x9");
  Object.defineProperty(plain, "lastIndex", { writable: false });
  try {
    p = plain.exec("y8");
    console.log("plain ok", p !== null ? p[1] : "null");
  } catch (e) {
    console.log("plain caught", e instanceof TypeError);
  }
}
run();
