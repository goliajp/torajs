// A parameter spelled like a `this`-reading function declaration
// shadows it inside that one function only: every other value use of
// the declaration still gets the receiver-carrying value face.

function thunk(this: any) {
  this.x = 1;
}

function throws3(ctor: any, thunk: any, msg: any = undefined): void {
  const kind = typeof thunk;
  try {
    thunk();
    console.log("no throw", kind);
  } catch (e) {
    console.log(e instanceof ctor, kind, msg);
  }
}
throws3(TypeError, thunk);

function takesNum(thunk: number, d: number = thunk * 2) {
  return thunk + d;
}
console.log(takesNum(2));

const o: any = {};
const f: any = thunk;
f.call(o);
console.log(o.x, f === thunk);
