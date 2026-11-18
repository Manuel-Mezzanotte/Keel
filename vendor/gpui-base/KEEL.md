# gpui-base 0.9.1, as Keel uses it

A copy of the published crate (`cargo` registry, version 0.9.1) with a few
additions Keel needs. Each change is marked `KEEL:` in the code:

- `editor/search.rs`: `SearchOptions` (case, whole word, regex),
  `InputBaseState::set_search_query_with`, `SearchMatcher::update_query_with`,
  `pattern_error`, `replacement_for` (`$1` in replacements); the replace
  functions use it.
- `editor/display_map/display_map.rs`: `DisplayMap::fold_candidates`.
- `base/state.rs`: `set_folded_at_line`, `unfold_all`, `selected_ranges`,
  `set_selected_ranges`, `visible_buffer_lines`, `gutter_width`.

To move to a new version: copy the new crate here again, redo the changes
above (search the old copy for `KEEL:`), and run Keel's tests.
