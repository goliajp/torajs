// A private element installed on an object `Object.preventExtensions`
// already marked is a TypeError (proposal nonextensible-applies-to-
// private) — on the class's OWN instance too, not only the foreign store
// of a return-override: a base ctor that seals `this` refuses the
// subclass's private field, method and accessor alike, and a base
// class whose own field initializer seals `this` refuses its own field.
// The typed instance takes the inline cursor path, so the refusal
// lives on both the kernel and the fast path.
class NonExtensibleBase { constructor(seal: boolean) { if (seal) Object.preventExtensions(this); } }
class WithField extends NonExtensibleBase { #val: number; constructor(seal: boolean) { super(seal); this.#val = 42; } val(): number { return this.#val; } }
class WithMethod extends NonExtensibleBase { constructor(seal: boolean) { super(seal); } #pm(): number { return 42; } pub(): number { return this.#pm(); } }
class WithAccessor extends NonExtensibleBase { constructor(seal: boolean) { super(seal); } get #pa(): number { return 42; } get pub(): number { return this.#pa; } }
function attempt(label: string, f: () => any): void { try { console.log(label, f()); } catch (e: any) { console.log(label, "TypeError", e instanceof TypeError); } }
attempt("field open", () => new WithField(false).val());
attempt("field sealed", () => new WithField(true).val());
attempt("method open", () => new WithMethod(false).pub());
attempt("method sealed", () => new WithMethod(true).pub());
attempt("accessor open", () => new WithAccessor(false).pub);
attempt("accessor sealed", () => new WithAccessor(true).pub);
class SelfSeal { #g: string = (Object.preventExtensions(this), "Test262"); g(): string { return this.#g; } }
attempt("self seal", () => new SelfSeal().g());
class Twice { #a = 1; constructor() { Object.preventExtensions(this); } }
attempt("seal after install", () => new Twice() instanceof Twice);
function mk(seal: boolean): any {
  class Local extends NonExtensibleBase { #x = 7; constructor() { super(seal); } x(): number { return this.#x; } }
  return new Local().x();
}
attempt("nested open", () => mk(false));
attempt("nested sealed", () => mk(true));
