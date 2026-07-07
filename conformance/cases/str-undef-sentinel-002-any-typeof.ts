// RFC 20260707-undefined-sentinel-repr chunk 3 — the sentinel
// crosses into the Any world as a real (Undef, 0) box (print / eq /
// typeof agree), and typeof on a nullable-str source answers the
// three-state runtime split instead of the static "string" fold.

const m = /a(b)?/.exec("a");
if (m !== null) {
  console.log(typeof m[1]);
  const c = m[1];
  console.log(typeof c);
  const a: any = m[1];
  console.log(a);
  console.log(a === undefined);
  console.log(a === null);
  console.log(a == null);
  console.log(typeof a);
  const arr: any[] = [m[1], "x"];
  console.log(arr[0]);
  console.log(arr[0] === undefined);
  const o: any = { f: m[1] };
  console.log(o.f);
  console.log(o.f === undefined);
}

// hit path: a real Str crosses unchanged
const h = /a(b)/.exec("ab");
if (h !== null) {
  console.log(typeof h[1]);
  const b: any = h[1];
  console.log(b, typeof b, b === undefined);
}
console.log("done");
