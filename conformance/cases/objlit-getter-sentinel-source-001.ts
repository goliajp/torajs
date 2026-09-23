// A getter written on an object literal hands back the sentinel just
// as one written on a class does. The class halves live in
// `accessor_getters` / `static_accessor_getters`, keyed by declaring
// class; a literal has no class, so the only record of the connection
// is the literal's own `__getter_` field, and nothing read it.
const zs: number[] = [1, 2, 3];

const src = {
  get g(): number {
    return zs[9];
  },
  get live(): number {
    return zs[0];
  },
};

console.log(typeof src.g, src.g === undefined);
console.log(typeof src.live, src.live);
console.log(src.g === undefined, src.live === undefined);

class Cls {
  get g(): number {
    return zs[9];
  }
}
console.log(typeof new Cls().g, new Cls().g === undefined);
