// Adapted from test262: language/expressions/template-literal/* —
// backtick string with `${}` interpolation. tr lexes templates as a
// single Token::Template carrying alternating Lit / Expr parts; the
// parser stitches them into a `+` chain at AST build, reusing the
// existing string-concat path with auto number→string coercion.
function check(): number {
  let n: number = 42;
  let name: string = "world";

  // Pure literal — should not allocate beyond the initial String.
  if (`hello` !== "hello") { throw "#1"; }

  // Single number interpolation.
  if (`n=${n}` !== "n=42") { throw "#2"; }

  // String + number interleaved.
  if (`hi, ${name}, n=${n}` !== "hi, world, n=42") { throw "#3"; }

  // Expression interpolation (binop, member access).
  let xs: number[] = [10, 20, 30];
  if (`len=${xs.length}` !== "len=3") { throw "#4"; }
  if (`sum=${1 + 2 + 3}` !== "sum=6") { throw "#5"; }

  // Adjacent interpolations (no literal between).
  if (`${n}${name}` !== "42world") { throw "#6"; }

  // Empty literal segments at the ends.
  if (`${n}` !== "42") { throw "#7"; }

  return 0;
}
console.log(check());
