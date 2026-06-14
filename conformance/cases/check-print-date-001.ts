// Nested-print substrate trunk Commit 5 acceptance — Tag::Date
// typed-receiver console.log. SSA dispatcher Type::Date → print_any
// → __torajs_print_anyv Tag::Date branch → __torajs_date_to_iso_string
// + put_str_cell_inline + rc_dec. Closes Date typed-receiver wedge.
const d = new Date(0);
console.log(d);
