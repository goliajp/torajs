// ES2024 §27.6 — `async *g() {}` in the two member positions. The
// substrate was already whole on both sides: a top-level
// `async function*` runs, and rotation 226 taught both member positions
// the plain `*g() {}` shape. All that was missing was the modifier and
// the star being allowed to stand next to each other.
//
// Consumed through `await it.next()` at the call site, which is as far
// as an async generator can currently be carried: `for await` routes to
// the async protocol only when its source is a direct call to a named
// factory, and the object does not survive a typed boundary. Neither
// limit is specific to member position — a top-level `async function*`
// hits both — so neither is exercised here.

class C {
  n = 10;
  async *g(k: number) {
    yield this.n + k;
    yield this.n - k;
  }
  static async *s() {
    yield 100;
  }
  // Teaching the `async` lookahead to step over a `*` meant teaching it
  // about private names as well, since `async *#p() {}` names one --
  // which incidentally admits the plain `async #m() {}` it had also been
  // refusing.
  async #hidden(): Promise<number> {
    return 41;
  }
  async reveal(): Promise<number> {
    return (await this.#hidden()) + 1;
  }
}

async function main(): Promise<void> {
  const c = new C();

  // Instance method — `this` reaches the body as the receiver parameter
  // that the hoisted `function*` takes.
  const it = c.g(2);
  console.log((await it.next()).value);
  console.log((await it.next()).value);
  console.log((await it.next()).done);

  // Static method — the receiver is the class object.
  const st = C.s();
  console.log((await st.next()).value);
  console.log((await st.next()).done);

  // Object-literal shorthand.
  const o = {
    async *m() {
      yield 7;
      yield 8;
    },
  };
  const om = o.m();
  console.log((await om.next()).value);
  console.log((await om.next()).value);
  console.log((await om.next()).done);

  // §27.6: the factory hands back the generator object directly rather
  // than a Promise of one, so the call itself needs no await.
  const direct = c.g(5);
  console.log((await direct.next()).value);

  console.log(await c.reveal());
}

main();
