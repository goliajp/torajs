// chunk 790 — member-path narrows and the named-fn member-assign
// wrap reach through element receivers: `arr[0].cb = g;
// if (arr[0].cb) { arr[0].cb() }` (int-literal indices only — a
// computed index write inside the guard conservatively kills the
// narrow, staying a loud reject).
type O = { cb?: () => number };
function g(): number { return 4 }
const arr: O[] = [{ cb: undefined }];
arr[0].cb = g;
if (arr[0].cb) { console.log(arr[0].cb()) } else { console.log("none") }
if (arr[0].cb) {
  arr[0].cb = undefined;
  console.log("assigned");
}
if (arr[0].cb) { console.log("bad") } else { console.log("cleared") }
arr[0].cb = () => 7;
const f = arr[0].cb;
if (f) { console.log(f()) }
console.log("end");
