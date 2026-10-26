// An `any` value returned from a function whose return type the
// program wrote as an object type literal is handed back as it is
// (§10.2.1.12; an annotation does nothing at run time): function
// declarations, arrows, class methods and getters, and functions nested
// in another function return it without a check, as bun does. An object
// literal's method still checks the value against the type and throws a
// TypeError for one that does not fit, so this case is checked against
// its .expected.

function attempt(tag: string, f: () => void): void {
  try {
    f();
    console.log(tag, "ok");
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message);
  }
}

function decl(src: string): { x: number } {
  return JSON.parse(src);
}
attempt("decl", () => console.log(decl('{"x":1}').x));
attempt("decl extra", () => decl('{"x":1,"y":2}'));
attempt("decl type", () => decl('{"x":"1"}'));
attempt("decl number", () => decl("3"));

// a value held in a binding first is no longer fresh: the return
// judges it as a return into a named object type does
function viaLet(src: string): { x: number } {
  const v: any = JSON.parse(src);
  return v;
}
type T = { x: number };
function viaLetNamed(src: string): T {
  const v: any = JSON.parse(src);
  return v;
}
attempt("via let", () => viaLet('{"x":1}'));
attempt("via let, named type", () => viaLetNamed('{"x":1}'));

const arrow = (src: string): { x: number } => JSON.parse(src);
attempt("arrow", () => console.log(arrow('{"x":2}').x));
attempt("arrow missing", () => arrow('{"y":2}'));

class C {
  m(src: string): { x: number } {
    return JSON.parse(src);
  }
  get g(): { x: number } {
    return JSON.parse('{"x":"no"}');
  }
}
attempt("method", () => console.log(new C().m('{"x":3}').x));
attempt("method null", () => new C().m("null"));
attempt("getter", () => new C().g);

const o = {
  m(src: string): { x: number } {
    return JSON.parse(src);
  },
};
attempt("object method", () => console.log(o.m('{"x":4}').x));
attempt("object method array", () => o.m("[]"));

function outer(k: number): number {
  const inner = (src: string): { x: number } => JSON.parse(k > 0 ? src : "{}");
  return inner('{"x":' + k + "}").x + inner('{"x":1}').x;
}
attempt("closure", () => console.log(outer(5)));
attempt("closure missing", () => outer(0));
function outerNested(k: number): number {
  function nested(src: string): { x: number } {
    return JSON.parse(src + "".padEnd(k));
  }
  return nested('{"x":' + k + "}").x + nested('{"x":true}').x;
}
attempt("nested", () => outerNested(6));

function maybe(src: string): { x: number } | undefined {
  return JSON.parse(src);
}
attempt("nullable", () => console.log(maybe('{"x":7}')?.x));
attempt("nullable undefined", () => console.log(maybe('{"x":7}') === undefined));
attempt("nullable number", () => maybe("8"));
