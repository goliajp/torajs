// Integration: switch / case with fallthrough, default, and string
// scrutinee. Exercises the M4-shape switch lowering (alloca'd result
// + chained CondBr).
function category(n: number): string {
  let r = "";
  switch (n) {
    case 0: r = "zero"; break;
    case 1:
    case 2:
    case 3:
      r = "small";
      break;
    case 100: r = "century"; break;
    default:
      r = n < 0 ? "negative" : "big";
  }
  return r;
}

function ascii_class(c: string): string {
  switch (c) {
    case "a":
    case "e":
    case "i":
    case "o":
    case "u":
      return "vowel";
    case " ":
      return "space";
    case "0":
    case "1":
    case "2":
    case "3":
    case "4":
    case "5":
    case "6":
    case "7":
    case "8":
    case "9":
      return "digit";
    default:
      return "other";
  }
}

function check(): number {
  if (category(0) !== "zero") { throw "#1"; }
  if (category(1) !== "small") { throw "#2"; }
  if (category(2) !== "small") { throw "#3"; }
  if (category(3) !== "small") { throw "#4"; }
  if (category(4) !== "big") { throw "#5"; }
  if (category(100) !== "century") { throw "#6"; }
  if (category(-7) !== "negative") { throw "#7"; }
  if (category(1000) !== "big") { throw "#8"; }

  if (ascii_class("a") !== "vowel") { throw "#9"; }
  if (ascii_class("e") !== "vowel") { throw "#10"; }
  if (ascii_class("z") !== "other") { throw "#11"; }
  if (ascii_class(" ") !== "space") { throw "#12"; }
  if (ascii_class("5") !== "digit") { throw "#13"; }
  if (ascii_class("?") !== "other") { throw "#14"; }
  return 0;
}
console.log(check());
