// rotation 275 刀 5 — §13.3.3 BindingRestElement : ... BindingPattern:
// an array pattern's rest slot may itself be a nested array / object
// pattern (`[...[x, y]]`), in declarations and for-await heads alike.
// The collected tail array becomes the nested pattern's source.

const [...[x, y, z]] = [3, 4, 5];
console.log(x, y, z);

const [a, ...[b, c]] = [1, 2, 3];
console.log(a, b, c);

const [...{ length }] = [7, 8];
console.log(length);

// defaults inside the nested rest pattern
const [p, ...[q = 10]] = [1];
console.log(p, q);

// recursion: rest-of-rest
let [...[m, ...[n]]] = [1, 2, 3];
console.log(m, n);

// elision before the rest pattern
const [, ...[e1, e2]] = [1, 2, 3];
console.log(e1, e2);

// empty nested rest pattern is legal and binds nothing
const [...[]] = [1, 2];
console.log("empty-ok");

// for-await head
async function fn() {
  for await (const [...[u, v]] of [[1, 2]]) {
    console.log(u, v);
  }
}
fn();
