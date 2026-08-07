# Must-dos

- Use `cargo run -p wq-cli -- --help` to understand CLI usage
  - e.g. `cargo run -p wq-cli -- exec 'inline code' -d ast,inst -p`
- Prefer `--box off` for machine-readable CLI probes so presentation layout and ANSI styling do not affect the observation.
  - e.g. `cargo run -p wq-cli -- --box off exec 'inline code' -p`
- Read `e/*.wq` to understand wq grammar
  - `lhs:rhs` is assignment
  - `a=b` is equality
  - list is `(1;2;3)`
    - wrong shape: `(1 2 3)`
  - call/index is `target[expr1;expr2...]`. Notice the brackets and semicolons
  - postfix:
    - `fn arg` calls
      - `fn1 fn2 arg` chains
      - wrong: `fn arg1 arg2`
    - `container index` indexes
  - `+` is broadcasting add
  - binary `,` concats
  - leading `,` enlists
  - `/` is classic division and int division produces floats. `/.` is exact division and preserves rational fractions when possible.
  - `^` is classic power; negative or fractional numeric exponents may produce floats/complex values. `^.` is exact power. Use exact operands such as `1/.3`, not `1/3`, when you need exact fractional exponents.
  - CAS simplification (`@s`, `cas_*`, `numeric_*`) should preserve exact constants where possible. It may use exact dot arithmetic internally even when the symbolic surface operator is `CasOp::Divide` or `CasOp::Power`.
    - If you add or change CAS integration strategies, update the unsupported integral reason classifier in `wqpl/src/cas/integrate.rs` and its tests so `unsupported symbolic integral` notes stay accurate.
  - `$[c;t;f;...]` is ternary. If false, every expression after the second semicolon belongs to the false branch, so `$[c;t;f1;f2]` runs `f1` then returns `f2` when `c` is false.
  - `$.[c;t1;t2...]` is a guard. It runs the body only when `c` is true; otherwise it returns an empty list `()`.
  - `$$[c1;t1;c2;t2;default]` is a condition chain. Conditions are checked in order. The final default is optional; an omitted default is an empty list.
  - `|` is pipe, which inserts lhs as the first arg to rhs call
  - `band[...]`, `bor[...]`, and `bxor[...]` apply eager bitwise logic to ints or bools.
  - `A[...]` and `O[...]` are short-circuit bool and/or forms.
  - `and[...]` and `or[...]` are parser aliases for `A[...]` and `O[...]`.
  - `(1)` is not a list. It is atom `1`.
  - comments: `//` `/* */`. `/ a` is division and not a comment.
  - quoted literals that decode to exactly one Unicode scalar are char atoms, so `"a"`, `"\n"`, and `"🦀"` are chars
    - other quoted literals are strings; use `,"a"` to create a one-character string
    - char atoms display as `"..."`; one-character strings display as `,"..."`
    - indexing a string returns char atoms, so compare an indexed character directly, for example `"abc" 0="a"`
    - hex escapes use exactly two digits, such as `\x41`; malformed forms such as `\x`, `\x4`, and `\xGG` are syntax errors
    - Unicode escapes use Rust-style `\u{...}` syntax with 1 to 6 hexadecimal digits
    - `\N{...}` accepts Unicode primary names, formal aliases, and approved named sequences
    - use `graphemes` when user-perceived characters rather than Unicode scalars matter
  - canonical value naming:
    - user-facing containers are `list` and `dict`
    - user-facing non-containers are atoms; do not call wq values scalars
    - `ValueCategory` and `Value::category()` are the stable public model
    - public categories are `int`, `float`, `complex`, `fraction`, `algebraic`, `char`, `tag`, `bool`, `list`, `cas`, `dict`, `function`, `rng`, and `stream`
    - both machine-width and bigint-backed ints have category `int`; all specialized list storage and strings have category `list`; compiled functions, closures, builtin-functions, and function compositions have category `function`
    - internal int storage details like `int` and `bigint` are both reported as `int` in user-facing messages and docs.
    - `ValueKind` and `Value::debug_kind()` are representation-oriented and distinguish `bigint`, `string`, `closure`, `builtin-function`, and `function-composition`
    - use `int-list`, `bool-list`, and `float-list` as debug-kind names in display, debugger/profiler output, tests, and code comments; do not use custom languages like `list<int>`, `list<bool>`, or `list<float>`
    - route public surfaces such as the REPL, language-server hover, WASM values, and diagnostic data through `category()`
    - route representation-oriented debugger, profiler, trace, DAP, and inspection surfaces through `debug_kind()`; label the field or column `kind`, not `type`
    - when a surface intentionally shows both, label them explicitly as `category` and `kind`
    - in user-facing docs, treat strings as part of the public list/container story unless text behavior is the point; the word "string" is fine when it helps clarity
    - do not describe strings as "lists of chars", "char lists", or "text" in user-facing docs; say "string"
    - do not use "list-like" in user-facing docs; use "container" when both list and dict are meant, or "list" when dicts are not included
    - in user-facing docs, prefer prose such as "list of ints" over compact type notation
    - reserve internal Rust storage names such as `Value::IntList`, `Value::BoolList`, `Value::FloatList`, and `Value::String` for implementation details
  - canonical builtin naming:
    - use `builtin` as the shorthand in compact UI labels, navigation, and prose
    - use `builtin-function` as the formal full term and debug kind; pluralize it as `builtin-functions`
    - do not use `bfn`, `built-in`, or `builtin function`
  - canonical diagnostics and requirements:
    - construct reusable expectations with `wqerror::Requirement` and finish them with `WqError::expected`; do not hand-build parallel `expected ...` strings when the model can express the requirement
    - keep articles and the word `expected` outside `Requirement`; requirement renderings must compose grammatically in singular, plural, union, list, dict, modifier, and bounded-range contexts
    - use `Requirement::one_of` for alternatives, `Requirement::list` and `Requirement::dict` for containers, `Requirement::int_range` with explicit inclusive/exclusive `Bound`s for int domains, modifiers for constraints such as positive/non-negative/finite, `string_literal` for string values, and `literal` only for canonical bare values
    - preserve exact range semantics in prose; for example, `(0,255]` is `int greater than 0 and at most 255`
    - nested alternatives must retain clear scope, such as `list whose elements are positive ints or positive floats`; complex dict members must use scoped wording such as `values that are lists of ints`
    - the public `int` category includes bigint-backed values; if an operation has a machine-width or other numeric limit, either accept bigint-backed ints that fit or state the real bounded requirement
    - never emit a self-contradictory diagnostic such as `expected int` followed by `got ... (int)` when the actual rejection is an unstated range or representation limit
    - standardize value context as `got VALUE (category)`, `got lhs VALUE (category)`, or `got rhs VALUE (category)` using excerpts and `category()`; do not add generic outer quotes around values
    - use one-based `at argument N`, `at named argument 'name'`, and precise zero-based element/index context where applicable
    - use the centralized arity helpers so messages include correct singular/plural grammar, the actual count, and builtin usage
    - quote identifiers, callable names, operators, and source syntax with single quotes
    - quote actual string values with double quotes, preferably through `Requirement::string_literal`; render tag values with canonical backtick syntax
    - use source lexemes or canonical diagnostic names instead of Rust `Debug` output for tokens, operators, and syntax
    - classify impossible compiler, VM, and CAS invariants as internal errors; do not expose helper names, storage widths, Rust types, or implementation shapes in user-facing messages
  - canonical consumer/API naming:
    - `\category` and `\category?` are the canonical REPL commands; `\type` and `\type?` may remain compatibility aliases
    - external structured values and bindings use a `category` field; debug-only structured values use `kind`
    - bump a versioned external diagnostic schema when renaming or changing its stable fields, and migrate all checked-in consumers and contract tests together
  - use `bool` consistently for wq values, operations, docs, diagnostics, tests, and UI copy
  - `@r expr` returns from a function.
  - `@s <expr>` creates a symbolic CAS structure.
    - After using `@s` to create one, apply operations directly instead of stacking `@s`.
      - e.g. `diff integrate @s 1/(x^3-2)`
    - A bare `x^3-2` without `@s` is evaluation and is not related to CAS.
  - postfix binds tighter than operators, `echo 1+2` <=> `(echo 1)+2` => prints `1`, evals to `()/*empty list*/+2` => evals to `2`. Does not evaluate to `3` and print `3`.
- Ensure `cargo clippy --all-targets -- -D warnings` passes.
  - Avoid using `#[allow(...)]` to pass clippy.
  - If passing clippy requires a large-scope edit, pause and ask the user.
- If you changed wq lexer/grammar:
  - Update `wq-ts/grammar.js`
  - Regenerate tree-sitter parser
  - Add new corpse tests
- Unless the user explicitly requested/approved, don't build with profile `release` or `R`.
- For perf-related tasks, use `hyperfine`.
- Avoid `panic!`. Prefer `unreachable!` or `debug_assert!`.
- Avoid `unwrap()`. Prefer `expect()`.
- Use `a.rs + a/` instead of `a/mod.rs` for modules with submodules.
  - Prefer no modifiers (i.e., private) over `pub(super)`.
  - Prefer `pub(super)` over `pub(crate)`.
  - Prefer `pub(crate)` over `pub`.
  - Avoid `pub` if it is not intended to be public API.
- If you touched Python code, run `ty check`, ruff lint and ruff format.
- If you changed `wqpl/viz`, also update `wqide/viz`.
- If you added/changed syntax feature or builtin, also update:
  - `wqpl/doc`
  - `d/articles/wqpl` if it is mentioned there
- No em dashes in comments/docs/code. Adjust wording to avoid em dashes.
- Use these newer Rust features when they make code cleaner:

| Feature         | Stabilized in | Release date | Notes                                                                                                 |
| --------------- | ------------- | ------------ | ----------------------------------------------------------------------------------------------------- |
| let chains      | Rust 1.88.0   | 2025-06-26   | Stable only in Rust 2024 edition. Allows `if let ... && let ... && condition` and similar in `while`. |
| `if let` guards | Rust 1.95.0   | 2026-04-16   | Stabilizes `if let` guards on `match` arms, e.g. `pat if let Some(x) = expr => ...`.                  |

## DRY and established helpers

- Before adding a local helper, repeated `match` over `Value`, or parallel implementation of a language rule, search the repository with `rg`. Reuse or extend the canonical helper when its contract fits. If several modules need a new rule, place it in the narrowest shared module and test it there. Do not generalize code that has only one specialized use.
- Use the storage and container abstractions in `wqpl/src/value/seq.rs`:
  - `ValueSeq` is a borrowed view over every public list representation, including strings and virtual int ranges. Use it for representation-independent length, indexing, gathering, iteration, and equality.
  - `ListStorageSeq` covers generic and packed list storage but deliberately excludes strings. Use it when list insertion or mutation must treat a string as one value unless a string-specific path applies.
  - `ExactIntSeq` handles int atoms, packed int lists, virtual int ranges, and exact general int lists without duplicating conversion and storage checks.
  - `ValueSeqBuilder` and `Value::from_items` select packed int, float, bool, or string storage when possible and widen to a general list when required. Use them for results assembled from `Value` items. Construct `Value::List` directly only when the result must stay in generic list storage.
- Use the existing `Value` predicates instead of repeating variant sets. The central predicates and classifications include `is_atom`, `is_list`, `is_unit`, `is_string`, `is_dict`, `is_container`, `is_runtime_callable`, `is_callable`, `category`, and `debug_kind` in `wqpl/src/value.rs`, plus numeric predicates in the relevant `wqpl/src/value/` modules.
- Use the constructor and conversion helpers in `wqpl/src/value.rs`, `wqpl/src/value/convert.rs`, and `wqpl/src/value/display.rs`. Established helpers include `Value::empty_list`, `Value::float`, `Value::from_bigint`, `Value::from_complex64`, `Value::from_fraction_parts`, `Value::from_items`, `IntoWqValue`, `into_wq_string`, and the `try_to_rust_*` methods. These preserve normalization, storage selection, and conversion semantics.
- Use `Value::bc1`, `Value::bc2`, their depth-aware variants, and `BcError` in `wqpl/src/value/bc.rs` for recursive broadcasting and path-aware failures. Use `wqpl/src/value/access.rs` for indexing and mutation semantics, and `Value::cat`, `Value::cat_many`, and `Value::flatten` in `wqpl/src/value/op/container.rs` for established container operations.
- Treat `wqpl/src/escape.rs` as the source of truth for wq escapes. Use `escape_string_inner`, `quote_string`, `unescape_string_inner`, and `valid_escape_sequence_len` instead of implementing escaping or escape validation in lexers, parsers, highlighters, formatters, or display code. Use `escape_str_for_display` when formatting a displayed wq string.
- Reuse the diagnostic helpers as part of DRY. Build domain expectations with `Requirement` and `WqError`; use the shared `expected_*` helpers in `wqpl/src/value/error.rs` when they fit. Builtins must use `check_arity`, `check_arity_named`, `check_arity_any_named`, `check_named_args`, `type_mismatch`, and declaration metadata in `wqpl/src/builtins.rs` instead of hand-built arity, named-argument, usage, or type errors.

## Documentation Markdown

- Render terminal Markdown unordered list markers and horizontal rules with ASCII `-`, not Unicode glyphs.
- When inline wq source contains backticks, use a CommonMark delimiter longer than every backtick run inside the source.
- If the source begins or ends with a backtick, add one padding space inside both delimiters so it renders without padding. For example:

  ```markdown
  `` `tag ``
  ``(`a:1;`b:2)``
  ```

- Do not omit representative tag or dict examples, or rewrite an example merely to avoid handling the delimiters. Verify the rendered Markdown whenever a code span contains backticks.

## Rust code format policy

- First check `cargo +nightly fmt --check` to ensure no surprising formatting happens
- Then use the format command: `cargo +nightly fmt`

## Audit policy

- `cargo deny check`
- Treat `multiple-versions` warnings as informational by default. Do not spend significant time trying to eliminate them, since duplicate versions are often caused by transitive dependency constraints and are not necessarily actionable.
  - Fix a warning only when the solution is obvious, low risk, and narrowly scoped, such as updating a direct dependency, refreshing the lockfile, or removing an unnecessary dependency. Do not add dependency overrides, patch transitive crates, downgrade unrelated packages, or make broad dependency changes solely to remove a warning.
  - If no straightforward fix is available, leave the warning in place and continue.

## Test policy

Choose between rust unit tests and hotchoco snapshot tests to lock behaviors or correctness.

- Snapshot tests use `hotchoco.py`.
  - This tests semantics, formatter, backtraces, etc.
  - If a new major module is added, create a new test config for it.
  - Key commands: `uv run hotchoco.py run`, `uv run hotchoco.py show --no-pager`, `uv run hotchoco.py accept --test TEST`.
  - See `uv run hotchoco.py --help` for details.

For all implemented changes, run the full:

- `cargo test -p wqpl`
- `cargo test -p wq-cli`
- `uv run hotchoco.py run`

Do not replace these with targeted runs at final handoff. Targeted runs are fine while iterating, but the final verification must include the full commands above unless the user explicitly says not to run them or an external blocker prevents them.

## wq-ts policy

Use the npm-managed local Tree-sitter CLI. Regenerate the parser with `npm run generate`; do not invoke a globally installed Tree-sitter CLI.

Add new corpus tests under `test/corpus`. After writing each test case, run `npm exec tree-sitter test -u`, review the generated expected parse tree for correctness, then run `npm exec tree-sitter test` without `-u` to verify the full suite.

Large changes to generated `parser.c` can be normal after grammar changes. Do not hand-edit generated files. Carefully inspect unexpectedly large changes to `node-types.json`, since they indicate changes to the grammar’s exposed node structure and should correspond to intentional grammar changes.

## Commit messages

At the final handoff only, recommend one commit message when this session produced an actual change that is ready to commit.

Do not recommend a commit message when:

- the user is only asking questions, brainstorming, debugging conceptually, or requesting an explanation;
- no files were changed;
- the change is only hypothetical, suggested, or not yet implemented;
- the user is continuing to polish a previous uncommitted change and the current response is not a final handoff.

When continuing an existing uncommitted change, update the previous recommendation only at the final handoff instead of emitting a new commit message after every follow-up.

When continuing a session but the previous turn's changes have already been committed, emit a new commit message instead of updating the previous one.

Use:

- Clear, capitalized, imperative title: `Fix everything`
- Avoid "conventional commit prefixes" (no `fix:`)
- Avoid trailing punctuation
- Optionally prefix the title with a crate name and colon when one crate is the clear scope: `wq-cli: did something`
  - Common prefixes:
    - `wqpl` for syntax/builtin changes
    - `wqpl/cas` for specific cas changes
    - `wq-cli` for specific cli changes
- Clear, consise message body. Do not manually hard-wrap with newlines when suggesting. If you are committing because user approved so, you may manually wrap.
- Include a `Release Notes:` section as the final section
- Use one bullet under `Release Notes:`:
  - `- Added ...` for a new user-facing capability;
  - `- Fixed ...` for a user-facing bug fix;
  - `- Improved ...` for a user-facing refinement to existing behavior;
  - `- N/A` for refactors, tests, tooling, formatting, docs-only changes, internal cleanup, dependency changes, CI/build changes, or any change with no direct user-visible effect.
  - Do not write release notes such as "Improved internals," "Improved maintainability," or "Improved tests." If the user cannot directly observe the change in product behavior, output `- N/A`.
- Format release notes exactly with a blank line after the heading
