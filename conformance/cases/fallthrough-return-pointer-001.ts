// The same fall-through path as `fallthrough-return-number-001`, but
// for the return widths that hold a pointer.
//
// These needed no new representation: a Str / Substr / fn / Obj / Arr
// / Closure slot already decodes three ways (NULL is `null`, the
// per-type immortal cell is `undefined`, anything else is live), which
// is what an optional field and a `find` miss have been handing out.
// The work was to hand the same cell back from the tail, and to teach
// the "may this hold the sentinel" predicates that calling one of
// these functions is another way to get one — that last part is a
// property of the callee, not of the width it arrives in.

class C {
  v: number = 1;
}

function str(f: boolean): string {
  if (f) {
    return "a";
  }
}

function arr(f: boolean): number[] {
  if (f) {
    return [1, 2];
  }
}

function inst(f: boolean): C {
  if (f) {
    return new C();
  }
}

console.log(str(true), str(false));
console.log(typeof str(true), typeof str(false));
console.log(str(false) === undefined, str(false) == undefined);
console.log("pre " + str(true));

console.log(arr(true), arr(false));
console.log(typeof arr(false));

console.log(inst(true), inst(false));
console.log(typeof inst(false));

// through a binding
const s = str(false);
console.log(s);
console.log(typeof s);

// through a finally hand-off
function guardedStr(f: boolean): string {
  try {
    if (f) {
      return "x";
    }
  } finally {
    console.log("cleanup");
  }
}
console.log(guardedStr(true));
console.log(guardedStr(false));

// every path returns — untouched
function fullStr(f: boolean): string {
  if (f) {
    return "y";
  }
  return "z";
}
console.log(fullStr(true), fullStr(false));
console.log(fullStr(false).length);
