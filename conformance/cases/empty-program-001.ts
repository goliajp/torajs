// rotation 497 — a program with no executable top-level statement
// (comment-only, or declarations only) must still link and exit 0:
// the entry calls main_user unconditionally, and main's synthesis used
// to bail on an empty top level (masked while every program carried
// the injected Error hierarchy's registration statements).
function f() {}
