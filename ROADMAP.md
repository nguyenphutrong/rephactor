# ROADMAP

## Phase 0: Project Skeleton

- [x] Create Rust binary crate.
- [x] Add LSP and PHP parser dependencies.
- [x] Document product scope and implementation plan.
- [x] Add CI once the first tests exist.

## Phase 1: LSP Baseline

- [x] Implement stdio LSP startup with `tower-lsp`.
- [x] Advertise `textDocument/codeAction` support.
- [x] Track open PHP documents.
- [x] Return no actions until parsing and selection mapping are ready.
- [x] Add unit tests for document state.

## Phase 2: Parser and Call Detection

- [x] Parse open PHP documents with `tree-sitter-php`.
- [x] Map LSP UTF-16 positions to byte offsets safely.
- [x] Detect function calls, method calls, static calls, and object creation.
- [x] Add fixtures for cursor/range inside call expressions.

## Phase 3: Same-file Function Conversion

- [x] Index same-file function declarations.
- [x] Resolve simple function calls in the current namespace.
- [x] Generate insertion-only edits for positional arguments.
- [x] Skip calls with unpacking, dynamic targets, or existing ambiguous named args.

## Phase 4: Project Index

- [x] Read `composer.json`.
- [x] Support PSR-4 namespace roots.
- [x] Index project classes, functions, methods, and constructors conservatively.
- [ ] Index traits and interfaces.
- [ ] Incrementally invalidate changed files beyond open-document overrides.

## Phase 5: Method and Constructor Resolution

- [x] Resolve instance methods when the receiver type is known locally.
- [x] Resolve static method calls.
- [x] Resolve constructors for `new ClassName(...)`.
- [ ] Handle inherited methods and implemented interfaces conservatively.

## Phase 6: Editor Integration

- [x] Add local Zed extension packaging or documented command setup.
- [ ] Verify Zed shows and applies the code action.
- [ ] Add end-to-end fixture or smoke test that exercises LSP JSON-RPC.

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
