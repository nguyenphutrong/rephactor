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
- [x] Invalidate cached indexes for watched disk changes outside open documents.

## Phase 11: Primary LSP Baseline

- [x] Document Rephactor's primary PHP LSP direction.
- [x] Implement `textDocument/signatureHelp` for resolved callables.
- [x] Implement `textDocument/definition`.
- [x] Implement import refactor code actions.
- [x] Implement hover for resolved symbols.
- [x] Link seeded PHP internal function hovers to the PHP manual.
- [x] Implement deterministic completion V1.
- [x] Implement PHP keyword completion.
- [x] Add conservative auto-import edits for class completions.
- [x] Add camel/underscore-aware matching for completion and workspace symbols.
- [x] Include indexed parent/interface/trait/mixin methods in completions.

## Phase 12: Intelephense Parity Foundations

- [x] Implement `textDocument/documentSymbol`.
- [x] Implement `workspace/symbol`.
- [x] Implement `textDocument/references`.
- [x] Implement `textDocument/declaration` for implemented methods.
- [x] Implement `textDocument/typeDefinition` for locally typed variables.
- [x] Implement `textDocument/implementation` for class/interface inheritance.
- [x] Implement `textDocument/implementation` for interface/base methods.
- [x] Implement `textDocument/rename` for exact AST symbol references.
- [x] Resolve calls imported with `use function`.
- [x] Implement reference-count `textDocument/codeLens` for declarations.
- [x] Implement code action for missing direct interface methods.
- [x] Implement code action for missing direct abstract parent methods.
- [x] Implement PHPDoc creation code actions for function-like declarations.
- [x] Add direct thrown-exception tags to PHPDoc creation.
- [x] Resolve local PHPDoc `@var` variable types.
- [x] Resolve inline local PHPDoc `@var Type` annotations for the following assignment.
- [x] Resolve PHPDoc `@param` parameter types.
- [x] Use PHPDoc `@param` parameter contracts in type mismatch diagnostics.
- [x] Resolve PHPDoc `@param self/static/parent` contracts in class-like scopes.
- [x] Resolve simple local variable aliases for instance method receivers.
- [x] Resolve assigned call-return types for instance method receivers.
- [x] Resolve native `self` and `static` parameter receiver types.
- [x] Resolve native direct `parent` parameter receiver types.
- [x] Resolve PHPDoc `@mixin` methods for instance method resolution.
- [x] Resolve PHPDoc `@method` magic methods.
- [x] Implement parser diagnostics for open files.
- [x] Implement callable-resolution diagnostics for open files.
- [x] Implement duplicate declaration diagnostics for open files.
- [x] Implement duplicate parameter diagnostics for open files.
- [x] Implement duplicate named-argument diagnostics for open files.
- [x] Implement unknown named-argument diagnostics for open files.
- [x] Implement too-many-argument diagnostics for open files.
- [x] Add conservative type metadata for seeded PHP internal functions.
- [x] Implement unused-import diagnostics for open files.
- [x] Implement `textDocument/documentHighlight`.
- [x] Implement `textDocument/foldingRange`.
- [x] Implement whitespace-only `textDocument/formatting`.
- [x] Implement whitespace-only `textDocument/rangeFormatting`.
- [x] Implement parameter-name `textDocument/inlayHint`.
- [x] Implement inferred return-type `textDocument/inlayHint`.
- [x] Infer return-type inlay hints from resolved calls and local variables.
- [x] Implement variable lookup `textDocument/inlineValue`.
- [x] Implement `textDocument/documentLink` for include/require paths.
- [x] Implement syntax-tree `textDocument/selectionRange`.
- [x] Implement unresolved type-annotation diagnostics for open files.
- [x] Implement conservative return-type mismatch diagnostics for open files.
- [x] Implement PHPDoc `@return` return-type mismatch diagnostics for open files.
- [x] Accept `null` for nullable native return and parameter type diagnostics.
- [x] Accept `null` for single-type `Type|null` native and PHPDoc diagnostics.
- [x] Implement local-variable return-type mismatch diagnostics for open files.
- [x] Implement returned-call return-type mismatch diagnostics for open files.
- [x] Implement assigned-call return-type mismatch diagnostics for open files.
- [x] Implement conservative argument-type mismatch diagnostics for open files.
- [x] Cover constructor argument-type mismatch diagnostics for open files.
- [x] Resolve native `self`, `static`, and direct `parent` parameter contracts in diagnostics.
- [x] Implement assigned-variable argument-type mismatch diagnostics for open files.
- [x] Implement returned-call argument-type mismatch diagnostics for open files.
- [x] Implement PHPDoc `@return` call argument-type mismatch diagnostics for open files.
- [x] Implement assigned-call argument-type mismatch diagnostics for open files.
- [x] Implement conservative assignment-type mismatch diagnostics for open files.
- [x] Implement returned-call assignment-type mismatch diagnostics for open files.
- [x] Implement PHPDoc `@var` assignment-type mismatch diagnostics for open files.
- [x] Implement PHPDoc `@var` returned-call assignment-type mismatch diagnostics for open files.
- [x] Implement local variable alias type-flow diagnostics for open files.
- [x] Implement broader type-flow semantic diagnostics beyond obvious assignments/arguments.
