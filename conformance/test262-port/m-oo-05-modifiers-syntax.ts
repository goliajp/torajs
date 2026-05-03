// Phase M-OO.5 — visibility modifier syntax + enforcement.
//
// `public` / `private` / `protected` / `readonly` modifiers parse as
// contextual keywords; check.rs enforces visibility based on the
// caller's class context (`__cm_<C>__*` / `__sm_<C>__*` fn name
// pattern → current_class). The binding's nominal class is stored
// on LocalInfo.declared_class when `let c: Counter = ...` matches a
// known class name in `ast.class_parents`.
//
// This case exercises the legal access patterns:
//   - public members read / called from outside the class
//   - private + protected accessed from `this` inside the class
//   - protected accessed from a subclass via `this`
// Negative-enforcement cases (private read from outside, etc.) are
// rejected at typecheck — they can't be expressed as conformance
// cases that pass three-way (the runner expects build success).

class Animal {
  protected name: string;
  private secret: number;

  constructor(n: string, s: number) {
    this.name = n;
    this.secret = s;
  }

  public greet(): string {
    return this.name;
  }

  private revealSecret(): number {
    return this.secret;
  }

  public reveal(): number {
    // Private access from inside the same class is fine.
    return this.revealSecret();
  }
}

class Dog extends Animal {
  constructor(n: string, s: number) {
    super(n, s);
  }

  public describe(): string {
    // Protected access from a subclass `this` is fine.
    return this.name;
  }
}

function check(): number {
  let a: Animal = new Animal("alice", 42);
  if (a.greet() !== "alice") { throw "#1: public method"; }
  if (a.reveal() !== 42) { throw "#2: private accessed via public method"; }

  let d: Dog = new Dog("rex", 7);
  if (d.greet() !== "rex") { throw "#3: inherited public method"; }
  if (d.describe() !== "rex") { throw "#4: protected via subclass this"; }
  if (d.reveal() !== 7) { throw "#5: private (via inherited public)"; }

  return 0;
}
console.log(check());
