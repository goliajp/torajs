// §10.1.8.1 / RFC 20260712 chunk 3 — `delete <Ctor>.prototype.<m>`
// leaves a tombstone, and the READ channel never asked about it. The
// four universal probes (hasOwnProperty / propertyIsEnumerable /
// valueOf / isPrototypeOf) plus toString / toLocaleString answer
// "supported" before any walk starts — which is what makes them
// reachable from every receiver shape, and equally what put them out
// of a tombstone's reach. So `delete Object.prototype.toString` left
// `typeof ({} as any).toString` answering "function".
//
// The prototype that SUPPLIES a name is the receiver's own family
// when that family owns it, and %Object.prototype% otherwise. That
// distinction is the whole answer, and it is why the matrix below
// has to be a matrix: after deleting toString from the root, `arr` /
// `num` / `date` keep theirs (their prototypes own one) while `obj` /
// `map` / `set` / `prom` lose it — and after deleting valueOf, `arr`
// loses it too, because Array.prototype owns no valueOf.
const anchor: any = Object
const names: string[] = ["toString", "valueOf", "toLocaleString", "hasOwnProperty",
                         "propertyIsEnumerable", "isPrototypeOf"]
function recvs(): any[] {
  return [{}, [], "s", 5, true, new Map(), new Set(), /r/, new Date(0),
          function () {}, Symbol("x"), 10n, Promise.resolve(1)]
}
function labels(): string[] {
  return ["obj", "arr", "str", "num", "bool", "map", "set", "re", "date",
          "fn", "sym", "big", "prom"]
}
function row(tag: string): void {
  const rs: any[] = recvs()
  const ls: string[] = labels()
  for (let i = 0; i < rs.length; i++) {
    let line: string = tag + " " + ls[i] + " :"
    for (let j = 0; j < names.length; j++) {
      line = line + " " + names[j] + "=" + typeof rs[i][names[j]]
    }
    console.log(line)
  }
}
row("pre ")
const O: any = Object.prototype
for (let j = 0; j < names.length; j++) {
  delete O[names[j]]
  row("d" + String(j))
}
