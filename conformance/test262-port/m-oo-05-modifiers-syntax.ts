// Phase M-OO.5 (partial — parser + AST only).
//
// `public` / `private` / `protected` / `readonly` modifiers are
// recognized as contextual keywords on class members. The visibility
// flag is stored on `ClassMethod` and in `Ast.member_visibility` /
// `Ast.readonly_fields`. Runtime enforcement is **deferred** — the
// typecheck layer needs nominal class-name info on `Type::Struct`
// (currently structural-only) to resolve `obj.member` back to its
// declaring class. Same framing as M-OO.3 vtable conversion.
//
// This case verifies that adding the modifiers doesn't break parsing
// or downstream lowering — every member can still be reached as if
// public, which preserves compatibility while the enforcement layer
// is built out.

class Counter {
  public count: number;
  private secret: number;
  protected tag: string;
  public readonly initial: number;

  constructor(start: number, t: string) {
    this.count = start;
    this.secret = start * 2;
    this.tag = t;
    this.initial = start;
  }

  public bump(): number {
    this.count = this.count + 1;
    return this.count;
  }

  private internal(): number {
    return this.secret + 1;
  }

  protected wrap(): string {
    return this.tag;
  }
}

function check(): number {
  let c: Counter = new Counter(10, "alpha");
  if (c.count !== 10) { throw "#1: public field"; }
  if (c.bump() !== 11) { throw "#2: public method"; }
  if (c.bump() !== 12) { throw "#3: bump again"; }
  // Note — these reads currently succeed because enforcement is
  // deferred. Once nominal class typing lands, M-OO.5 enforcement
  // will reject the next two lines (case will be split into a
  // positive-syntax test + a negative-enforcement test).
  if (c.secret !== 20) { throw "#4: private field still readable (parser-only)"; }
  if (c.internal() !== 21) { throw "#5: private method still callable (parser-only)"; }
  if (c.tag !== "alpha") { throw "#6: protected field still readable (parser-only)"; }
  if (c.wrap() !== "alpha") { throw "#7: protected method still callable (parser-only)"; }
  if (c.initial !== 10) { throw "#8: readonly field"; }
  return 0;
}
console.log(check());
