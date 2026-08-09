// RFC 20260810 knife 1 fix — the self-name lives in a scope of its
// OWN between the enclosing environment and the body (§15.5.5
// funcEnv): a body-level `let n` / `var n` re-declaration is a legal
// shadow (scope-lex-open / scope-var-open), and a zero-capture
// self-named body may lower before its construction site
// (for-head destructuring defaults / export default) without
// consulting the capture side-channel.
var n: any = 'outside';
var probeBefore: any = function () { return n; };
var probeInside: any;
var func: any = function n() {
  let n: any = 'inside';
  probeInside = function () { return n; };
};
func();
console.log(probeBefore(), probeInside());
var probeBody: any;
var func2: any = function n() {
  var n: any;
  probeBody = function () { return n; };
};
func2();
console.log(probeBody());
