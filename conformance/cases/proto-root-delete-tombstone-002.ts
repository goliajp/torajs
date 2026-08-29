// The other direction of the supplier rule, and the reason it has to
// be a chain rather than a single owner: deleting a name from a
// FAMILY prototype does not make the read undefined — the walk
// continues to %Object.prototype%, which still has one. The read
// goes undefined only once the root's copy is gone too, which is the
// state 001 never reaches from this side. A restore revives without
// any clear call, because a dynobj own entry is probed before the
// tombstone is consulted.
//
// WHICH function answers after a family-only delete is the call
// channel's half of the same question, and the row below now pins it:
// the walk steps past the tombstone and %Object.prototype%'s own
// toString is what runs. The three names a family can both own and
// share with the root get the wider matrix in
// `proto-root-family-delete-redirect-001`.
const anchor: any = Object
const A: any = Array.prototype
console.log("pre  typeof :", typeof ([] as any).toString)
console.log("pre  value  :", ([1, 2] as any).toString())
delete A.toString
console.log("fam  typeof :", typeof ([] as any).toString)
console.log("fam  value  :", ([1, 2] as any).toString())
console.log("fam  obj    :", typeof ({} as any).toString, ({} as any).toString())
console.log("fam  join    :", typeof ([] as any).join)
delete (Object.prototype as any).toString
console.log("both typeof :", typeof ([] as any).toString)
console.log("both obj    :", typeof ({} as any).toString)
A.toString = function () { return "restored" }
console.log("back typeof :", typeof ([] as any).toString)
console.log("back value  :", ([1, 2] as any).toString())
console.log("back obj    :", typeof ({} as any).toString)
