bug(S17): serde content-rescan corrupts NodeKind::Struct.name

Root cause confirmed via runtime debug:
  NodeKind::Struct { name: 'User', fields: [{name: 'name'}, {name: 'score'}] }
  deserializes at runtime as name='generic_clamp' (a field value from a
  *different* node's nested Field objects).

Mechanism:
  serde_json's TaggedContentVisitor buffers the entire JSON object as a
  Content tree for externally-tagged enum variants. When #[serde(default)]
  fields are present (attrs, where_clauses, struct_kind on NodeKind::Struct),
  serde re-scans the Content tree per-field to find matches. On re-scan it
  can escape the outer object boundary and pick up 'name' keys from nested
  Field objects -- or from adjacent nodes in the buffer.

  This is a known serde issue with externally-tagged enums + #[serde(default)]
  + nested structs sharing field names with the outer variant.
  Ref: serde#1183 / serde_json content deserializer field aliasing.

Impact:
  NodeKind::Struct.name reads the wrong string value at runtime.
  invariant_solver fails: Impl node 12 references unknown struct 'User'
  because 'User' never appears in impl_target_names.

Proposed fixes (to be evaluated):
  Option A: #[serde(rename = 'field_name')] on Field::name + JSON migration
  Option B: Replace NodeKind serde with manual Deserialize impl
  Option C: Add a capture::validate() layer that round-trips NodeKind
             through serde_json::Value first and checks field presence
             before handing IR to analyze()
  Option D: Use #[serde(deny_unknown_fields)] + rename all nested 'name'
             fields to break the collision

Next step: implement Option C as the validation layer, then pick
permanent structural fix.
