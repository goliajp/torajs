// A moved async body holding a `new Promise(executor)` local: the
// machine field is a `Promise<any>` (the bare class name was an
// unknown type to the field checker). The executor hands `resolve`
// out so the body can suspend until the test releases it — the shape
// of every "not disposed while suspended" case.
async function f(): Promise<any> {
  var release: any;
  var p = new Promise(function (resolve: any) { release = resolve; });
  var q = new Promise((resolve: any) => { resolve(2); });
  await null;
  release(7);
  return (await p) + (await q);
}
f().then((v: any) => console.log("new-promise-local", v));
