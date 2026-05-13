# ROADMAP

## Phase 0: Project Skeleton

- [x] Create Rust binary crate.
- [x] Add LSP and PHP parser dependencies.
- [x] Document product scope and implementation plan.
- [ ] Add CI once the first tests exist.

## Phase 1: LSP Baseline

- Implement stdio LSP startup with `tower-lsp`.
- Advertise `textDocument/codeAction` support.
- Track open PHP documents.
- Return no actions until parsing and selection mapping are ready.
- Add unit tests for document state.

## Phase 2: Parser and Call Detection

- Parse open PHP documents with `tree-sitter-php`.
- Map LSP UTF-16 positions to byte offsets safely.
- Detect function calls, method calls, static calls, and object creation.
- Add fixtures for cursor/range inside call expressions.

## Phase 3: Same-file Function Conversion

- Index same-file function declarations.
- Resolve simple function calls in the current namespace.
- Generate insertion-only edits for positional arguments.
- Skip calls with unpacking, dynamic targets, or existing ambiguous named args.

## Phase 4: Project Index

- Read `composer.json`.
- Support PSR-4 namespace roots.
- Index project classes, functions, methods, constructors, traits, and interfaces.
- Incrementally invalidate changed files.

## Phase 5: Method and Constructor Resolution

- Resolve instance methods when the receiver type is known locally.
- Resolve static method calls.
- Resolve constructors for `new ClassName(...)`.
- Handle inherited methods and implemented interfaces conservatively.

## Phase 6: Editor Integration

- Add local Zed extension packaging or documented command setup.
- Verify Zed shows and applies the code action.
- Add end-to-end fixture or smoke test that exercises LSP JSON-RPC.

## Phase 7: Broader PHP Semantics

- Add PHP internal function stubs.
- Handle Composer classmaps.
- Consider PHPStan/Psalm metadata for type resolution.
- Add configuration for project PHP version.

## Phase 8: Release Hardening

- Add CI for formatting, clippy, tests, and fixture snapshots.
- Add release binaries for macOS, Linux, and Windows.
- Publish installation instructions.
- Document known unsupported cases.
