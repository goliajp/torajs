// §16.2.2 AttributeKey is `IdentifierName | StringLiteral`, and
// IdentifierName admits every reserved word. tr's with-clause parser
// carried its own two-arm list (Ident | String), so every keyword
// spelling was a parse error — the fifth position in the parser to
// grow a private property-name list, after the four
// `keyword_property_name` was created to unify.
//
// The `type` key is the one with a host meaning, and tr reads every
// module as source, so it stays a loud refusal rather than a silently
// ignored clause; the keys below have no defined effect and pass
// through.
import { VALUE } from "./import-attribute-keyword-key-001-lib.ts" with {
  if: "a",
  for: "b",
  class: "c",
  function: "d",
  new: "e",
  typeof: "f",
  instanceof: "g",
  default: "h",
  yield: "i",
  async: "j",
};
console.log(VALUE);
