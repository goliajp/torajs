// a module that rebinds the global parseInt calls what it bound; the
// record lane does not read through the name

globalThis.parseInt = (x: string, r?: number): number => x.length * 100 + (r === undefined ? 0 : r);
const re = /(\d+)(x)?/g;
let m = re.exec("7 42x 999");
while (m !== null) {
  console.log(parseInt(m[1], 10), m[2] === undefined ? "-" : parseInt(m[2], 36), parseInt(m[1]));
  m = re.exec("7 42x 999");
}
