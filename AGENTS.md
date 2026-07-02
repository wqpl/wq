# Must-dos

- Use `cargo run -p wq-cli -- --help` to understand CLI usage
  - eg. `cargo run -p wq-cli -- exec 'inline code' -d ast,inst -p`
- Read `e/*.wq` to understand wq grammar
  - `lhs:rhs` is assignment
  - `a=b` is equality
  - list is `(1;2;3)`
    - wrong: `(1 2 3)`
  - call/index is `target[expr1;expr2...]`. Notice the brackets and semicolons
  - postfix:
    - `fn arg` calls
      - `fn1 fn2 arg` chains
      - wrong: `fn arg1 arg2`
    - `container index` indexes
  - `+` is broadcasting add
  - binary `,` is cat
  - leading `,` is enlist
  - `/` is classic division and integer division produces floats. `/.` is exact division and preserves rational fractions when possible.
  - `^` is classic/runtime power; negative or fractional numeric exponents may produce floats/complex values. `^.` is exact power. Use exact operands such as `1/.3`, not `1/3`, when you need exact fractional exponents.
  - CAS simplification (`@s`, `cas_*`, `numeric_*`) should preserve exact constants where possible. It may use exact dot arithmetic internally even when the symbolic surface operator is `CasOp::Divide` or `CasOp::Power`.
    - If you add or change CAS integration strategies, update the unsupported integral reason classifier in `wqpl/src/cas/integrate.rs` and its tests so `unsupported symbolic integral` notes stay accurate.
  - `$[c;t;f]` is ternary. If false, every expression after the second semicolon belongs to the false branch, so `$[c;t;f1;f2]` runs `f1` then returns `f2` when `c` is false.
  - `$.[c;t1;t2...]` is a guard. It runs the body only when `c` is true; otherwise it returns unit `()`.
  - `$$[c1;t1;c2;t2;default]` is a condition chain. Conditions are checked in order. The final default is optional; omitted default is unit.
  - `|` is pipe, which inserts lhs as the first arg to rhs call
  - `\` or `bor[...]` (backslash) is bitwise or.
  - `\|` (backslash pipe) is short-circuit bool or.
  - `or[...]` is eager bool or.
  - `(1)` is not a list. It is simply atom `1`
  - comments: `//` `/* */`
  - canonical value naming:
    - user-facing containers are `list` and `dict`
    - user-facing non-containers are atoms; do not call wq values scalars
    - use `list<int>`, `list<bool>`, and `list<float>` in display, debug/profiler output, tests, and code comments
    - in user-facing docs, treat strings as part of the public list/container story unless text behavior is the point; the word "string" is fine when it helps clarity
    - do not describe strings as "lists of chars", "char lists", or "text" in user-facing docs; say "string"
    - do not use "list-like" in user-facing docs; use "container" when both list and dict are meant, or "list" when dicts are not included
    - in user-facing docs, prefer prose such as "list of ints" over compact type notation
    - reserve internal Rust storage names such as `Value::IntList`, `Value::BoolList`, `Value::FloatList`, and `Value::String` for implementation details
  - `@r expr` is return
  - `@s <expr>` creates a symbolic CAS structure.
    - After using `@s` to create one, apply operations directly instead of stacking `@s`.
      - e.g. `diff integrate @s 1/(x^3-2)`
    - A bare `x^3-2` without `@s` is evaluation and is not related to CAS.
  - postfix binds tighter than operators, `echo 1+2` <=> `(echo 1)+2` => prints `1`, evals to `()/*unit*/+2` => evals to `2`. Does not evaluate to `3` and print `3`.
- Ensure `cargo clippy --all-targets -- -D warnings` passes.
  - Avoid using `#[allow(...)]` to pass clippy.
    - An exception is dead code that won't be used anymore, where you are allowed to use `#[allow(...)]` instead of removing it
  - If you can't pass clippy by fixing code for any reason, ask the user whether it's fine to use `#[allow(...)]`
  - If passing clippy requires a large-scope edit, pause and ask the user.
- format command: `cargo +nightly fmt`
- If you changed wq lexer/grammar:
  - Also update `wq-ts/grammar.js`, and
  - verify it with `tree-sitter generate`, and
  - add a new corpse test using `tree-sitter test -u`
- Delevopment should be test-driven. Choose between unit tests and snapshot tests depending on situation.
  - Integration/snapshot tests use `hotchoco.py`.
    - This tests semantics, formatter, backtraces, etc.
    - `python3 hotchoco.py run`, when you touched:
      - lexer/parser/compiler/vm/interpreter
      - anything that affects semantics
      - a `e/*.wq` script
      - formatter
    - If a new major module is added, you may create a new test config for it.
    - Key commands: `python3 hotchoco.py run`, `python3 hotchoco.py show --no-pager`, `python3 hotchoco.py accept --test TEST`.
    - See `python3 hotchoco.py --help` for details.
  - Test policy: prefer broader tests rather than only focused ones, e.g.
    - Full `tree-sitter test` if you changed `grammar.js`
    - Full `cargo test -p wqpl` if you changed `wqpl`
    - But usually avoid full workspace `cargo test` unless necessary
- At handoff, recommend a good commit message based on the appendix guidelines.
  - Do not commit unless the user explicitly asked for it.
- Unless the user explicitly requested/approved, don't build/run with `release` profile.
- Prohibited without explicit user permission:
  - Python/Perl... scripts (especially regex-based replacements) for batch editing
  - `sed`, `awk`, or any similar text-processing utilities for code changes
  - `git checkout`, `git restore`, `git reset`, or any other destructive git mutations
  - `cargo clean`, or any other destructive cargo commands
  - Any cargo commands that force a complete rebuild
  - `rm`. prefer `trash` instead.
- If you are given a perf-related task, prefer `hyperfine` over `time`
- When you are not sure of the user's intent, prefer asking the user instead of guessing.
- Avoid `panic!` outside tests. Prefer `unreachable!` or `debug_assert!` instead.
- Avoid `unwrap()`. Prefer `expect()`.
- Use `a.rs + a/` instead of `a/mod.rs` for modules with submodules.
  - Prefer no modifiers (i.e., private) over `pub(super)`.
  - Prefer `pub(super)` over `pub(crate)`.
  - Prefer `pub(crate)` over `pub`.
  - Avoid `pub` if it is not intended to be public API.
- If you touched Python code, run ruff lint and format.
- If you changed `wqpl/viz`, also update `wqide/viz`.
- If you added/changed syntax feature or builtin, also update:
  - `wqpl/doc`
  - optionally `d/articles/wqpl` if it is core to the language
- No em dashes in comments/docs/code. Adjust wording to avoid em dashes.
- Use these newer Rust features when they can make code cleaner:

| Feature         | Stabilized in | Release date | Notes                                                                                                 |
| --------------- | ------------- | ------------ | ----------------------------------------------------------------------------------------------------- |
| let chains      | Rust 1.88.0   | 2025-06-26   | Stable only in Rust 2024 edition. Allows `if let ... && let ... && condition` and similar in `while`. |
| `if let` guards | Rust 1.95.0   | 2026-04-16   | Stabilizes `if let` guards on `match` arms, e.g. `pat if let Some(x) = expr => ...`.                  |

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

- Clear, uncapitalized, imperative title: `fix everything`
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
- Format release notes exactly with a blank line after the heading, for example:
