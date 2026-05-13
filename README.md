# Rephactor

Rephactor is a PHP language server focused on semantic refactoring and
project-aware editor intelligence. It started as a companion server for a narrow
PHP 8 named-arguments refactor and is evolving toward a primary PHP language
server alternative.

The intended editor setup is to keep a full PHP language server such as
Intelephense or Phpactor for completion, diagnostics, hover, and navigation,
then run this server alongside it for a narrow code action:

```php
dispatch($order, true);
```

```php
dispatch(order: $order, notify: true);
```

This project grows in small verified slices. The named-arguments workflow
remains the correctness baseline while primary-LSP capabilities are added on top
of the same resolver and project index.

## Initial Scope

- Expose an LSP server over stdio.
- Provide `textDocument/codeAction` for call expressions under the cursor.
- Resolve safe, unambiguous PHP functions, methods, static methods, and
  constructors.
- Return `WorkspaceEdit` changes that insert `parameter_name: ` before
  positional arguments.
- Provide conservative class import refactor code actions.
- Provide conservative PHPDoc creation code actions for function-like
  declarations.
- Provide `textDocument/signatureHelp` for resolved callables.
- Provide `textDocument/definition` for resolved project symbols.
- Provide `textDocument/declaration` for implemented methods.
- Provide `textDocument/typeDefinition` for locally typed variables.
- Provide `textDocument/implementation` for class/interface inheritance.
- Provide `textDocument/hover` for resolved symbols.
- Provide `textDocument/rename` for exact AST symbol references.
- Provide `textDocument/codeLens` reference counts for declarations.
- Provide deterministic `textDocument/completion` for basic symbols.
- Add conservative `use` declaration edits for unambiguous class completions.
- Provide `textDocument/documentSymbol` for outline and breadcrumbs.
- Provide `workspace/symbol` for Composer-indexed project symbols.
- Provide `textDocument/references` for exact AST symbol references.
- Publish parse diagnostics for open PHP documents.
- Provide `textDocument/documentHighlight` for same-file symbol highlights.
- Provide `textDocument/foldingRange` for PHP blocks and comments.
- Provide parameter-name `textDocument/inlayHint` hints for resolved calls.
- Provide `textDocument/documentLink` for literal include/require paths.
- Provide syntax-tree `textDocument/selectionRange` expansion.
- Skip cases where conversion could change behavior or where symbol resolution
  is ambiguous.

## Non-goals

- Providing formatting or full static analysis.
- Converting dynamic calls before semantic resolution is robust.
- Guessing parameter names from text when the callable cannot be resolved.

## Development

```sh
cargo fmt --check
cargo check
cargo test
```

## Install

Install the language server binary into Cargo's global bin directory:

```sh
cargo install --path .
which rephactor
```

Zed must be able to find `rephactor` on `PATH`. If `which rephactor` does not
return a path, add Cargo's bin directory to your shell profile:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

Tagged releases publish prebuilt macOS, Linux, and Windows binaries through the
release workflow. During active development, `cargo install --path .` remains
the fastest local install path.

## Zed Setup

Zed compiles dev extensions to Wasm. Make sure Zed sees the Rustup toolchain
before Homebrew Rust, then install the Wasm target:

```sh
rustup target add wasm32-wasip2
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --manifest-path zed-extension/Cargo.toml --target wasm32-wasip2
```

The repository also includes the same check as a script:

```sh
scripts/check-zed-extension.sh
```

If `which cargo` prints `/opt/homebrew/bin/cargo`, move `$HOME/.cargo/bin`
earlier in your shell `PATH`. Otherwise Zed may fail to install the dev
extension with `can't find crate for core` for `wasm32-wasip2`.

Install the local extension from `zed-extension/` with Zed's
`zed: install dev extension` command.

Keep a full PHP language server enabled for normal language intelligence and
run Rephactor alongside it:

```json
{
  "languages": {
    "PHP": {
      "language_servers": ["intelephense", "rephactor", "..."]
    }
  }
}
```

Rephactor currently provides named-argument and class-import refactor code
actions, PHPDoc creation V1, Signature Help V1, Go To Definition V1, Go To
Declaration V1, Hover V1, Completion V1, Go To Type Definition V1, Go To
Implementation V1, Document Symbol V1, Workspace Symbol V1, References V1,
Diagnostics V1, Document Highlight V1, Rename V1, Folding Range V1, Inlay Hint
V1, Document Link V1, and Selection Range V1. Code Lens V1 shows declaration
reference counts.
The code action is titled `[Rephactor] Add names to arguments` when multiple
identifiers can be inserted. When only one positional argument is missing a
name, the title names that identifier, for example
`[Rephactor] Add name identifier 'exchange_gift'`.

Signature Help V1 shows parameter names for resolved functions, static methods,
constructors, and locally obvious instance methods. It intentionally returns no
signature for unsupported or ambiguous calls instead of guessing.

Go To Definition V1 navigates to resolved functions, classes, methods, static
methods, constructors, traits, interfaces, and imports that are present in the
project index. It returns no location for dynamic or ambiguous symbols.

Go To Declaration V1 navigates from a class method implementation to the
matching interface or base-class method declaration when that relationship is
unambiguous.

Go To Type Definition V1 navigates from locally typed variables and parameters
to the resolved class definition when the type can be inferred from a parameter
or nearby object creation assignment.

Go To Implementation V1 returns indexed classes that directly or transitively
extend or implement the class/interface under the cursor. On interface/base
method declarations, it returns matching implementation methods from derived
classes.

Hover V1 shows a concise PHP signature or class FQN, source location, and the
nearest PHPDoc summary when available. Hover for seeded PHP internal functions
links to the official PHP manual. It intentionally avoids full PHPDoc rendering
and returns no hover for ambiguous or dynamic symbols.

Completion V1 returns deterministic prefix, camel-case, and underscore-aware
matches for indexed class names, indexed project functions, seeded PHP internal
functions, static methods after `ClassName::`, and instance methods when the
receiver type is locally obvious. Method completion includes indexed parent,
interface, trait, and PHPDoc `@mixin` methods. It also includes common PHP
keyword completions and adds a `use` declaration edit for unambiguous namespaced
class completions when the short name is not already imported or shadowed. It
intentionally avoids snippets and fuzzy ranking.

Document Symbol V1 returns functions, classes, interfaces, traits, and class
methods for editor outline and breadcrumb UIs.

Workspace Symbol V1 searches Composer-indexed functions, classes, and methods
with deterministic case-insensitive, camel-case, and underscore-aware matching.

References V1 finds exact matching AST name references across Composer-indexed
PHP files and open document overlays. It is intentionally conservative and does
not yet perform full type-aware disambiguation.

Rename V1 returns a workspace edit for exact AST symbol references. It does not
rename files or folders.

Code Lens V1 shows exact-reference counts for function, class, interface,
trait, and method declarations.

Implement Interface Methods V1 offers a code action on classes with directly
implemented indexed interfaces and inserts stubs for missing interface methods.
Implement Abstract Methods V1 does the same for direct abstract parent methods.

PHPDoc creation V1 adds `@param` tags for function and method parameters and
an `@return` tag when a non-void return type is declared. It also adds
`@throws` tags for directly detected `throw new ExceptionClass(...)`
expressions.

PHPDoc Type V1 reads function-like `@param Type $variable` annotations and
local `@var Type $variable` annotations, then feeds them into method resolution
for completion, signature help, and code actions. Class-level `@method`
annotations are indexed as magic methods for the same features.

Diagnostics V1 publishes parser error diagnostics, unresolved/ambiguous
callable diagnostics, unresolved type-annotation diagnostics, and duplicate
function/class-like declaration diagnostics for open documents. It also reports
duplicate parameter diagnostics, duplicate/unknown named-argument diagnostics
for resolved calls, too-many-argument diagnostics for resolved non-variadic
calls, and conservative unused-import diagnostics for normal non-aliased class
imports. It also reports conservative return-type mismatches when a declared
return type conflicts with a directly returned scalar literal, array literal, or
object creation expression, including local variables assigned one of those
obvious values before return, resolved calls with declared return types, and
variables assigned from those calls.
Resolved calls also report conservative argument type mismatches when typed
parameters receive obvious literal or object-creation arguments, including
variables assigned obvious values earlier in the same local or top-level scope
and variables assigned from resolved calls with declared return types.
Direct resolved-call arguments with declared return types are checked too.
Typed parameters report assignment mismatches when
reassigned to those obvious values or resolved calls with declared return types,
and local `@var` PHPDoc annotations are used as assignment type contracts for
the same conservative checks. Broader static analysis is still deferred until
the type model is stronger.

Document Highlight V1 highlights exact matching AST names in the current
document.

Folding Range V1 folds PHP declaration blocks, compound statements, imports,
and comments from the syntax tree.

Formatting V1 trims trailing spaces/tabs for whole-document and range
formatting. Whole-document formatting also ensures a final newline. PSR-12
structural formatting is still deferred.

Inlay Hint V1 shows parameter names for resolved positional call arguments. It
also shows conservative return-type hints for function-like declarations without
a declared return type when they consistently return `new ClassName(...)`.

Inline Value V1 returns PHP variable lookup ranges for debugger inline values.

Document Link V1 links literal relative `include`/`require` paths to files on
disk.

Selection Range V1 returns syntax-tree ancestor ranges for smart selection
expansion.

Import refactors support adding an import for a resolvable fully-qualified
class name, shortening that usage, sorting simple class imports, and removing
unused simple class imports. Function imports, const imports, and destructive
grouped-import rewrites are intentionally skipped.

Zed currently applies LSP code actions directly. It does not show a PHPStorm-
style diff preview when moving through code action menu items; the aside popover
only expands long action titles.

When Rephactor returns no action, it logs the reason through the LSP client.
In Zed logs, look for lines like:

```text
Rephactor codeAction file:///path/File.php:2739:28 -> 0 action(s) in 4ms (index cache hit /path): unresolved callable customer_supplier::accumulatePoints
```

Project symbols are cached per Composer root after the first request. Open PHP
documents still override cached disk symbols. `workspace/didChangeWatchedFiles`
notifications invalidate the matching Composer-root cache so disk changes in
files that are not open in the editor are picked up on the next request.

## Supported Cases

- Same-file functions.
- Basic class, function, static method, and locally obvious instance method
  completions.
- Related instance method completions from indexed parents, interfaces, traits,
  and PHPDoc `@mixin` classes.
- PHP keyword completions.
- Document symbols for functions, class-like declarations, and methods.
- Workspace symbols for indexed functions, classes, and methods.
- Exact AST references across Composer-indexed PHP files.
- Exact AST symbol rename edits across Composer-indexed PHP files.
- Exact-reference code lenses for declarations.
- Method declaration lookup for interface/base-class implementations.
- Type definitions for locally typed variables and parameters.
- Class/interface implementation lookup across indexed PHP files.
- Interface/base method implementation lookup across indexed PHP files.
- Parse diagnostics for open PHP documents.
- Unresolved and ambiguous callable diagnostics for open PHP documents.
- Unresolved type-annotation diagnostics for open PHP documents.
- Duplicate function and class-like declaration diagnostics for open PHP
  documents.
- Duplicate function and method parameter diagnostics for open PHP documents.
- Duplicate named-argument diagnostics for resolved calls in open PHP documents.
- Unknown named-argument diagnostics for resolved calls in open PHP documents.
- Too-many-argument diagnostics for resolved non-variadic calls in open PHP
  documents.
- Conservative unused-import diagnostics for normal non-aliased class imports in
  open PHP documents.
- Conservative return-type mismatch diagnostics for directly returned literals,
  object creation expressions, and local variables assigned those obvious values
  before return.
- Conservative return-type mismatch diagnostics for returned resolved calls with
  declared return types.
- Conservative return-type mismatch diagnostics for variables assigned from
  resolved calls with declared return types.
- Conservative argument-type mismatch diagnostics for resolved calls with typed
  parameters and obvious literal, object-creation, or previously assigned
  variable arguments.
- Conservative argument-type mismatch diagnostics for resolved-call arguments
  with declared return types.
- Conservative argument-type mismatch diagnostics for variables assigned from
  resolved calls with declared return types.
- Conservative assignment-type mismatch diagnostics for typed parameters
  reassigned to obvious literal or object-creation values.
- Conservative assignment-type mismatch diagnostics for typed parameters
  reassigned from resolved calls with declared return types.
- Conservative assignment-type mismatch diagnostics for local PHPDoc `@var`
  variables assigned obvious literal or object-creation values.
- Same-file document highlights for exact AST name matches.
- Folding ranges for PHP blocks, imports, and comments.
- Whole-document and range whitespace formatting for trailing whitespace; whole
  documents also get a final newline.
- Parameter-name inlay hints for resolved positional call arguments.
- Inferred return-type inlay hints for function-like declarations that return
  one `new ClassName(...)` type.
- Inline value variable lookups for PHP variables in debugger ranges.
- Document links for literal relative include/require paths.
- Syntax-tree selection ranges.
- Conservative class import refactors for normal `use Foo\Bar;` declarations.
- Code action to implement missing methods from directly implemented indexed
  interfaces.
- Code action to implement missing methods from directly extended abstract
  classes.
- PHPDoc creation for function and method declarations with parameter and
  return tags, plus direct `throw new ExceptionClass(...)` throws tags.
- Local PHPDoc `@var Type $variable` annotations for instance method
  resolution.
- Function-like PHPDoc `@param Type $variable` annotations for instance method
  resolution.
- PHPDoc `@mixin ClassName` annotations for class instance method resolution.
- PHPDoc `@method` annotations for class magic method resolution.
- Namespaced same-file functions.
- Static methods and constructors when the class is indexed, including class
  names imported with normal, grouped, or aliased `use` declarations.
- Instance methods when the receiver type is locally obvious from a typed
  parameter or `$var = new ClassName(...)`.
- Project symbols under Composer `autoload.psr-4` roots.
- Project symbols under Composer `autoload.classmap` files or directories.
- A small seed set of PHP internal functions, such as `str_replace`,
  `json_encode`, `preg_match`, and `in_array`, with PHP manual links in hover.
- Calls that already contain safe named arguments and still have remaining
  positional arguments. Rephactor inserts only the missing names.
- Projects without a Composer PHP version constraint, or projects whose
  `require.php` constraint requires PHP 8 or newer.

## Unsupported Cases

Rephactor returns no action instead of guessing for:

- dynamic calls such as `$fn(...)` or `$object->$method(...)`
- calls with unpacking (`...$args`)
- function imports, const imports, and grouped-import rewrites
- calls whose existing named arguments do not match the resolved signature
- ambiguous symbols
- unknown parameter names
- PHP internal functions outside the seeded stub set
- completion for dynamic receivers or unresolved classes
- PSR-12 structural formatting beyond trailing-whitespace cleanup
- static-analysis diagnostics beyond parser, callable-resolution, and
  conservative return/argument/assignment type mismatch errors
- Composer autoload modes other than `autoload.psr-4` and `autoload.classmap`
- parent/interface/trait resolution that depends on unindexed or ambiguous
  symbols
- PHPStan/Psalm metadata, including generics, template annotations, and
  framework-specific dynamic return type extensions
- Composer `require.php` constraints that allow PHP 7.x, because named
  arguments require PHP 8

## Release Status

Current release posture:

- Build from source with `cargo install --path .`.
- Install the local Zed extension from `zed-extension/`.
- Verify the Rust server with `cargo fmt --check`, `cargo check`,
  `cargo test`, and `cargo clippy -- -D warnings`.
- Verify the Zed extension with `scripts/check-zed-extension.sh`.

Release binaries are built by GitHub Actions when a `v*` tag is pushed.

Deferred until V1 behavior is stable:

- full PHP internal stubs
- PHPStan/Psalm metadata

## Manual Acceptance

1. Install the binary with `cargo install --path .`.
2. Install the local Zed extension from `zed-extension/`.
3. Open a PHP project that has Composer PSR-4 autoloading.
4. Keep Intelephense or Phpactor enabled and add `rephactor` to the PHP
   `language_servers` list.
5. Put the cursor inside a supported call expression and run code actions.
6. Apply the Rephactor code action and verify that only `parameter_name: `
   prefixes were inserted.
7. Put the cursor inside an unsupported call and verify that Zed logs include a
   concise no-action reason.
8. Trigger signature help inside a supported call and verify that the active
   parameter follows the cursor.
