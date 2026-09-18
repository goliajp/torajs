// The loop that evaluates `outer` binds each class object to a `const`
// spelled like the class itself. That binding is not the class, so its
// readers are not the class's outside references — the class keeps its
// per-evaluation identity (§15.7.14) and its own `.name`.
function outer() {
  class K {
    #v = 1;
    get #x() { return this.#v; }
    set #x(n: number) { this.#v = n; }
    bump() { this.#x = this.#x + 1; return this.#x; }
  }
  return K;
}
let t = 0;
for (let i = 0; i < 20; i++) { const K = outer(); t += new K().bump(); }
console.log(t, outer() === outer());
const K = outer();
console.log(K.name, new K() instanceof K, new K() instanceof outer(), String(K).slice(0, 7));
function other() {
  class P { m() { return 7 } }
  return P;
}
for (const P of [other(), other()]) console.log(new P().m(), P === other());
