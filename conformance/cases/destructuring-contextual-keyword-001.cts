// A destructuring shorthand `{ x }` binds the name it reads, so it is
// legal exactly when that name is a valid BindingIdentifier. `async`
// and `type` are on nobody's reserved list — `async` is a keyword only
// where the grammar looks ahead for it (`async function`, `async (x)
// =>`) and `type` is TypeScript's, in declaration position only — so
// both shorthand like any other name.
//
// tr demanded the explicit `{ async: binding }` rename for them, the
// treatment a genuinely reserved word needs because a reserved word
// cannot be the BINDING. 45 test262 cases were refused at the pattern,
// 40 of them on `type` alone.

var { async, type } = { async: 1, type: 2 };
console.log("shorthand", async, type);

var { async: renamedAsync, type: renamedType } = { async: 3, type: 4 };
console.log("rename", renamedAsync, renamedType);

function takesPattern({ async, type }: any) {
  return async + type;
}
console.log("param", takesPattern({ async: 5, type: 6 }));

var { async: defaultedAsync = 7, type: defaultedType = 8 } = {} as any;
console.log("default", defaultedAsync, defaultedType);

var [{ async: nestedAsync }] = [{ async: 10 }];
console.log("nested", nestedAsync);

// A genuinely reserved word still cannot be the binding, so it still
// needs the rename — that is the line this must not move.
var { if: renamedIf } = { if: 9 };
console.log("reserved-rename", renamedIf);
