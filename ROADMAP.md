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
- [x] Fill missing names when safe named arguments already exist.

## Phase 4: Project Index

- [x] Read `composer.json`.
- [x] Support PSR-4 namespace roots.
- [x] Index project classes, functions, methods, and constructors conservatively.
- [x] Resolve normal, grouped, and aliased class imports.
- [x] Index traits and interfaces.
- [x] Use changed open project documents when building the project index.

## Phase 5: Method and Constructor Resolution

- [x] Resolve instance methods when the receiver type is known locally.
- [x] Resolve static method calls.
- [x] Resolve constructors for `new ClassName(...)`.
- [x] Handle inherited methods, implemented interfaces, and traits conservatively.

## Phase 6: Editor Integration

- [x] Add local Zed extension packaging or documented command setup.
- [x] Verify Zed shows and applies the code action.
- [x] Add a repeatable Zed extension Wasm check.
- [x] Add end-to-end fixture or smoke test that exercises LSP JSON-RPC.

## Phase 7: Broader PHP Semantics

- [x] Add an initial seed set of PHP internal function stubs.
- [x] Handle Composer classmaps.
- [x] Consider PHPStan/Psalm metadata for type resolution.
- [x] Add configuration for project PHP version.

## Phase 8: Release Hardening

- [x] Add CI for formatting, check, clippy, tests, and Zed extension Wasm check.
- [x] Add release binaries for macOS, Linux, and Windows.
- [x] Publish installation instructions.
- [x] Document known unsupported cases.

## Phase 9: Real-Project Reliability

- [x] Add skip-reason logging for no-action code action requests.
- [x] Expand JSON-RPC smoke fixtures for real supported and unsupported cases.
- [x] Keep unsupported cases as empty LSP action lists.

## Phase 10: Index Performance

- [x] Cache disk project indexes by Composer root.
- [x] Overlay open document text on top of cached disk symbols.
- [x] Log index cache hit/miss and code-action elapsed time.
- [ ] Add file watching for disk changes outside open documents.

## Phase 11: Primary LSP Baseline

- [x] Document Rephactor's primary PHP LSP direction.
- [x] Implement `textDocument/signatureHelp` for resolved callables.
- [x] Implement `textDocument/definition`.
- [x] Implement import refactor code actions.
- [x] Implement hover for resolved symbols.
- [x] Implement deterministic completion V1.

## Phase 12: Intelephense Parity Foundations

- [x] Implement `textDocument/documentSymbol`.
- [x] Implement `workspace/symbol`.
- [ ] Implement `textDocument/references`.
- [ ] Implement diagnostics for open files.
