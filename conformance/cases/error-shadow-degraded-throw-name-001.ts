// RFC 20260825-injection-reachability 刀 A — a user `class Error`
// shadows the whole injected hierarchy, so a runtime TypeError raise
// rides the bare-string fallback. The fallback now bakes the class
// name in (`TypeError: <msg>`), so the caught value's rendered first
// segment matches what bun's real instance answers. Only the name
// segment is compared — the message text is engine-specific.
class Error {
  note: string;
  constructor(note: string) {
    this.note = note;
  }
}

const n: any = null;
try {
  n.foo();
} catch (e) {
  console.log(String(e).split(":")[0]);
}

// The shadow class itself still works as declared.
console.log(new Error("mine").note);
