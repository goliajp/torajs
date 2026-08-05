// RFC 20260805-async-fn-state-machine D0, third half — the three
// initializer shapes the shared sniff cannot answer where the lift
// stands in the pipeline. Each pinned a generator local to `number`
// and took every later use of it down.
//
// A: `new C()` — a built-in and a user class. The shared sniff has no
//    `New` arm at all, so `const s = new Set()` inside a `function*`
//    did not compile: "field is Number, value is Set", then "no member
//    `.add` on type Number".
// B: calling a `function*`. Its declared return type describes what it
//    YIELDS, not what calling it answers — that is the iterator object.
//    Reading the annotation pinned `const it = ag()` to the yield type
//    and every `it.next()` failed.
// C: `undefined`. JS's untyped slot is `any`, and `number` made the
//    difference observable — the local printed `0`.

class Greeter {
  name: string = "tr";
  greet(): string {
    return "hi " + this.name;
  }
}

function* counter(): any {
  yield 1;
  yield 2;
  return 0;
}

function* g(): any {
  const set = new Set();
  set.add(7);
  set.add(7);
  yield set.size;

  const greeter = new Greeter();
  yield greeter.greet();

  const it = counter();
  yield it.next().value;
  yield "mid";
  yield it.next().value;
  yield it.next().done;

  const nothing = undefined;
  yield nothing;

  return 0;
}

function drain(gg: any): void {
  let r: any = gg.next();
  while (r.done === false) {
    console.log(r.value);
    r = gg.next(0);
  }
}

drain(g());
