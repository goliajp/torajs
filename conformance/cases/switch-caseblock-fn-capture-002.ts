// a clause keeps a declaration that captures its own let and moves one
// that captures only outer bindings; the moved body's block-level let
// shadows a moved sibling's name
function moved(c: number): string {
  let x = 1
  switch (c) {
    case 0:
      var r = g()
      function h() { return x }
      function g() { return () => k }
      let k = 2
      return h() + " " + r()
  }
  return "none"
}

function shadowed(c: number): string {
  let x = 1
  switch (c) {
    case 0:
      function h(): any { if (x) { let g = 2; return g } return 0 }
      function g() { return x }
      return String(h()) + " " + g()
  }
  return "none"
}

console.log(moved(0))
console.log(shadowed(0))
