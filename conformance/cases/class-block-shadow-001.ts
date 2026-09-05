// A block that declares a name of its own shadows a class of that
// name for exactly that block. The class-reference rewrite walked
// function scopes only, so at program level nothing shadowed at all
// and a block-scoped binding was read straight through to the class.
class C {
  static tag = "outer";
}
{
  let C: any = 42;
  console.log(C);
}
console.log(C.tag);

// The class needs no declaration of its own at program level for the
// hole to open — one hoisted out of any block is a program-level name
// all the same.
{
  class D {
    static tag = "d";
  }
  console.log(D.tag);
}
{
  let D: any = "shadowed";
  console.log(D);
}

// Shadowed by a class expression carrying the same name, whose own
// body still sees itself.
{
  const E: any = class Inner {
    field: any = Inner;
  };
  console.log(new E().field === E, E.name);
}

// The loop head, the catch body and the CaseBlock are scopes too.
for (let C = 1; C < 2; C++) {
  console.log(C);
}
try {
  throw 1;
} catch (e) {
  let C: any = "catch";
  console.log(C, e);
}
switch (1) {
  case 1:
    let C: any = "case";
    console.log(C);
    break;
}
console.log(C.tag);
