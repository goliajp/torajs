// §14.12.4 selects a clause with IsStrictlyEqual, which is a total
// function: it answers `false` across types rather than being undefined
// for them. So a `case` whose type cannot equal the scrutinee's is not
// an error — it is a clause that never gets selected, and the spec says
// so in the ordinary way.
//
// A typed scrutinee makes that decidable at compile time, which is the
// whole point: bun has to run the comparison to find out, tr already
// knows. Either way the observable behaviour is identical, and that is
// what this fixture pins.

let s: string = "a";
switch (s) {
    case null:
        console.log("null branch");
        break;
    case 0:
        console.log("zero branch");
        break;
    case "a":
        console.log("a branch");
        break;
    default:
        console.log("default");
}

let n: number = 0;
switch (n) {
    case "0":
        console.log("str zero");
        break;
    case 0:
        console.log("num zero");
        break;
}

// A clause that can never be selected still evaluates its expression —
// CaseClauseIsSelected runs the Expression before comparing, so a side
// effect in a never-matching case is observable.
let hits: number = 0;
function bump(): string {
    hits = hits + 1;
    return "nope";
}
let k: number = 1;
switch (k) {
    case (bump() as any):
        console.log("impossible");
        break;
    case 1:
        console.log("one");
        break;
}
console.log("never-matching clause still evaluated:", hits);
