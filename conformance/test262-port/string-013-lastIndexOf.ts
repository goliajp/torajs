// Adapted from test262: built-ins/String/prototype/lastIndexOf/* —
// reverse-direction substring scan; -1 on miss. Subset is single-arg
// (no fromIndex). Empty needle returns the receiver's length per
// JS spec.
function check(): number {
  if ("hello world hello".lastIndexOf("hello") !== 12) { throw "#1"; }
  if ("hello".lastIndexOf("hello") !== 0) { throw "#2: equal"; }
  if ("abc".lastIndexOf("a") !== 0) { throw "#3: single hit at 0"; }
  if ("aaa".lastIndexOf("a") !== 2) { throw "#4: last of repeats"; }
  if ("foo".lastIndexOf("zzz") !== -1) { throw "#5: miss"; }
  if ("foo".lastIndexOf("oo") !== 1) { throw "#6"; }

  // Substring at end.
  if ("hello world".lastIndexOf("world") !== 6) { throw "#7"; }

  // Empty needle returns length.
  if ("hello".lastIndexOf("") !== 5) { throw "#8: empty needle"; }
  if ("".lastIndexOf("") !== 0) { throw "#9: both empty"; }

  // Needle longer than haystack.
  if ("a".lastIndexOf("abc") !== -1) { throw "#10: too long"; }
  return 0;
}
console.log(check());
