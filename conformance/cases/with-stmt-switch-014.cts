// §14.11 + §14.12.4 — a `switch` in a `with` body.
//
// A guarded read is a conditional over the object, so the scrutinee is
// `any` whatever the object holds. That made every ordinary `case 1:`
// a type error here, which is how the interaction surfaced.
//
// `.cts` because `with` only exists under the sloppy goal.

var o: any = { pick: 2, name: "two" };

with (o) {
  switch (pick) {
    case 1:
      console.log("one");
      break;
    case 2:
      console.log("two");
      break;
    default:
      console.log("none");
  }
  switch (name) {
    case "two":
      console.log("named two");
      break;
    default:
      console.log("unnamed");
  }
}
