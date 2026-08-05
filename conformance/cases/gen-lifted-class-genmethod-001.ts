// A generator local holding the iterator a class's generator method
// answers.
//
// A generator method parses into two things: a hoisted top-level
// `function* __cm_gen_C__m(__genrecv, ..)` and an ordinary forwarder
// method that calls it. The hoisted half is what the generator
// desugar turns into the `__Gen_*` iterator class, so by the time a
// generator body's lets are lifted, the name it answers is already
// known — what was missing was the receiver's class, and `const b =
// new Box()` supplies it now.
//
// Without that, `const held = b.each()` fell to the `number` fallback
// every lifted local without an annotation used to take, and the
// checker — which types the call correctly — rejected the store:
// "field is Number, value is ClassRef(__Gen___cm_gen_Box__each)".
// Two locals apart, since the second one has to read a `binds` entry
// the first one wrote.

class Box {
  items: number[] = [1, 2, 3];
  *each(): Generator<number> {
    for (const v of this.items) {
      yield v;
    }
  }
  *twice(): Generator<number> {
    for (const v of this.items) {
      yield v * 2;
    }
  }
  size(): number {
    return this.items.length;
  }
}

function* outer(): number {
  const b = new Box();
  const held = b.each();
  for (const a of held) {
    yield a;
  }
  const doubled = b.twice();
  for (const d of doubled) {
    yield d;
  }
  // an ordinary method on the same receiver keeps the behaviour it
  // had — the hoisted generator spelling simply does not exist for
  // it, so the lookup declines and the fallback stands
  yield b.size();
}

for (const x of outer()) {
  console.log(x);
}
