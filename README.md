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

Go To Definition V1 navigates to resolved functions, constants, class constants
declared on indexed classes or related parents/interfaces/traits, including
`self::`, `static::`, and direct `parent::` references, classes, methods, static
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

Hover V1 shows a concise PHP signature, class FQN, constant FQN, or class
constant label, source location, and the nearest PHPDoc summary plus tag lines
when available. Hover for seeded PHP internal functions links to the official
PHP manual. Seeded internal functions include conservative parameter and return
metadata for simple scalar/array contracts. It intentionally avoids rich PHPDoc
rendering and
returns no hover for ambiguous or dynamic symbols.

Completion V1 returns deterministic prefix, camel-case, and underscore-aware
matches for indexed class names, indexed project functions and constants,
seeded PHP internal functions, static methods and class constants from indexed
classes and related parents/interfaces/traits after `ClassName::`, and instance
methods and properties when the receiver type is locally obvious, flows through
a simple local variable alias, or comes from a resolved call with a class return
type. Static scope completion understands `self`, `static`, and direct `parent`
inside class-like scopes. Method and property completion includes indexed
parent, interface, trait, and PHPDoc `@mixin` members. It also includes common
PHP keyword completions and adds a `use` or `use const` declaration edit for
unambiguous namespaced class
and constant completions when the short name is not already imported or
shadowed. It intentionally avoids snippets and fuzzy ranking.

Document Symbol V1 returns functions, constants, classes, interfaces, traits,
and class constants, properties, and methods for editor outline and breadcrumb
UIs.

Workspace Symbol V1 searches Composer-indexed functions, constants, classes,
and methods with deterministic case-insensitive, camel-case, and
underscore-aware matching.

References V1 finds exact matching AST name references for functions,
constants, classes, methods, and variables across Composer-indexed PHP files
and open document overlays. It is intentionally conservative and does not yet
perform full type-aware disambiguation.

Rename V1 returns a workspace edit for exact AST symbol references, including
constants. When a class-like declaration is renamed from a matching
`ClassName.php` file, it also adds a file rename operation for the PHP file.
Folder renames are still skipped.

Code Lens V1 shows exact-reference counts for function, constant, class,
interface, trait, method, and property declarations. Class-like and method
declarations also show indexed implementation counts when derived classes are
known.

Implement Interface Methods V1 offers a code action on classes with directly
implemented indexed interfaces and inserts stubs for missing interface methods.
Implement Abstract Methods V1 does the same for direct abstract parent methods.

PHPDoc creation V1 adds `@param` tags for function and method parameters and
an `@return` tag when a non-void return type is declared. It also adds
`@throws` tags for directly detected `throw new ExceptionClass(...)`
expressions.

PHPDoc Type V1 reads function-like `@param Type $variable` annotations, local
`@var Type $variable` annotations, and inline local `@var Type` annotations for
the following assignment, then feeds them into method resolution for completion,
signature help, code actions, and conservative diagnostics. Class-level
`@method` annotations are indexed as magic methods for the same features.
Function-like PHPDoc `@param` annotations understand `self`, `static`, and
direct `parent` inside class-like scopes.

Diagnostics V1 publishes parser error diagnostics, unresolved/ambiguous
callable diagnostics, unresolved type-annotation diagnostics, and duplicate
function/class-like declaration diagnostics for open documents. It also reports
duplicate method, property, and class-constant diagnostics inside class-like
declarations, duplicate parameter diagnostics, duplicate/unknown named-argument
diagnostics for resolved calls, too-many-argument diagnostics for resolved
non-variadic calls, and conservative unused-import diagnostics for normal
non-aliased class imports. It also reports conservative return-type mismatches
when a native or
PHPDoc-declared return type conflicts with a directly returned scalar literal,
array literal, or object creation expression, including local variables assigned
one of those obvious values before return, resolved calls with declared return
types, and variables assigned from those calls.
Nullable native `?Type` and single-type `Type|null` declarations accept `null`
for these conservative checks.
Resolved calls also report conservative argument type mismatches when typed
parameters receive obvious literal or object-creation arguments, including
variables assigned obvious values earlier in the same local or top-level scope
and variables assigned from resolved calls with declared return types. Those
known local variable types are propagated through simple variable-to-variable
assignments in the same forward scope.
Native `self`, `static`, and direct `parent` parameter contracts are resolved
for these checks inside class-like scopes.
Direct resolved-call arguments with declared return types are checked too.
PHPDoc `@return` annotations are used as declared return types for these
resolved-call checks.
Typed parameters report assignment mismatches when
reassigned to those obvious values or resolved calls with declared return types,
and local `@var` PHPDoc annotations are used as assignment type contracts for
the same conservative checks, including assignments from resolved calls with
declared return types. Typed `$this->property` assignments are checked against
native property declarations in the containing class for the same conservative
assignment flows. Broader static analysis is still deferred until the type model
is stronger.

Document Highlight V1 highlights exact matching AST names in the current
document.

Folding Range V1 folds PHP declaration blocks, compound statements, imports,
heredoc/nowdoc strings, comments, and custom `#region`/`#endregion` regions.

Formatting V1 trims trailing spaces/tabs for whole-document and range
formatting. Whole-document formatting also ensures a final newline. PSR-12
structural formatting is still deferred.

Inlay Hint V1 shows parameter names for resolved positional call arguments. It
also shows conservative return-type hints for function-like declarations without
a declared return type when returned expressions infer to one type, including
object creation expressions, local variables, and resolved calls.

Inline Value V1 returns PHP variable lookup ranges for debugger inline values.

Document Link V1 links literal relative `include`/`require` paths, including
`__DIR__` concatenated literals, to files on disk.

Selection Range V1 returns syntax-tree ancestor ranges for smart selection
expansion.

Import refactors support adding an import for a resolvable fully-qualified
class or constant name, shortening that usage, sorting simple class imports,
and removing unused simple class imports. Function imports and destructive
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
- Project constant completions.
- Class constant completions after `ClassName::`.
- Static-scope completion after `self::`, `static::`, and direct `parent::`.
- Instance property completions from indexed classes, parents, traits, and
  PHPDoc `@mixin` classes.
- Related instance method completions from indexed parents, interfaces, traits,
  and PHPDoc `@mixin` classes.
- PHP keyword completions.
- Document symbols for functions, constants, class-like declarations, class
  constants, properties, and methods.
- Workspace symbols for indexed functions, constants, classes, and methods.
- Exact AST references across Composer-indexed PHP files.
- Exact AST symbol rename edits across Composer-indexed PHP files.
- Matching class-like PHP file rename operations for class/interface/trait
  declaration renames.
- Class constant hover and definition lookup for direct, inherited, `self::`,
  `static::`, and direct `parent::` constant references.
- Exact-reference and implementation-count code lenses for declarations,
  including constants and properties.
- Method declaration lookup for interface/base-class implementations.
- Type definitions for locally typed variables and parameters.
- Class/interface implementation lookup across indexed PHP files.
- Interface/base method implementation lookup across indexed PHP files.
- Parse diagnostics for open PHP documents.
- Unresolved and ambiguous callable diagnostics for open PHP documents.
- Unresolved type-annotation diagnostics for open PHP documents.
- Duplicate function and class-like declaration diagnostics for open PHP
  documents.
- Duplicate method diagnostics for open PHP documents.
- Duplicate property diagnostics for open PHP documents.
- Duplicate class-constant diagnostics for open PHP documents.
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
- Conservative return-type mismatch diagnostics for PHPDoc `@return`
  declarations.
- Nullable native return-type diagnostics that accept `null` for `?Type` and
  `Type|null`.
- Conservative return-type mismatch diagnostics for returned resolved calls with
  declared return types.
- Conservative return-type mismatch diagnostics for variables assigned from
  resolved calls with declared return types.
- Conservative argument-type mismatch diagnostics for resolved calls with typed
  parameters and obvious literal, object-creation, or previously assigned
  variable arguments.
- Conservative argument-type mismatch diagnostics for resolved constructor
  calls.
- Conservative argument-type mismatch diagnostics for native `self`, `static`,
  and direct `parent` parameter contracts.
- Conservative argument-type mismatch diagnostics for PHPDoc `@param`
  parameter contracts.
- Conservative argument-type mismatch diagnostics for PHPDoc `@param self`,
  `@param static`, and direct `@param parent` contracts.
- Nullable native and PHPDoc parameter diagnostics that accept `null` for
  `?Type` and `Type|null`.
- Conservative argument-type mismatch diagnostics for seeded PHP internal
  function parameter contracts.
- Conservative argument-type mismatch diagnostics for resolved-call arguments
  with declared return types.
- Conservative argument-type mismatch diagnostics for resolved-call arguments
  with PHPDoc `@return` types.
- Conservative argument-type mismatch diagnostics for variables assigned from
  resolved calls with declared return types.
- Conservative argument-type mismatch diagnostics for local variable aliases of
  known obvious or resolved-call return types.
- Conservative assignment-type mismatch diagnostics for typed parameters
  reassigned to obvious literal or object-creation values.
- Conservative assignment-type mismatch diagnostics for typed parameters
  reassigned from resolved calls with declared return types.
- Conservative assignment-type mismatch diagnostics for typed parameters
  reassigned from local variable aliases of known types.
- Conservative assignment-type mismatch diagnostics for PHPDoc `@param`
  parameter contracts.
- Conservative assignment-type mismatch diagnostics for local PHPDoc `@var`
  variables assigned obvious literal or object-creation values.
- Conservative assignment-type mismatch diagnostics for local PHPDoc `@var`
  variables assigned from resolved calls with declared return types.
- Conservative assignment-type mismatch diagnostics for `$this->property`
  assignments when the containing class has a native property type.
- Same-file document highlights for exact AST name matches.
- Folding ranges for PHP blocks, imports, heredoc/nowdoc strings, comments, and
  custom `#region`/`#endregion` regions.
- Whole-document and range whitespace formatting for trailing whitespace; whole
  documents also get a final newline.
- Parameter-name inlay hints for resolved positional call arguments.
- Inferred return-type inlay hints for function-like declarations that return
  one obvious or resolved type.
- Inline value variable lookups for PHP variables in debugger ranges.
- Document links for literal relative include/require paths and `__DIR__`
  concatenated literals.
- Syntax-tree selection ranges.
- Conservative class import refactors for normal `use Foo\Bar;` declarations.
- Conservative constant import refactors for normal `use const Foo\BAR;`
  declarations.
- Code action to implement missing methods from directly implemented indexed
  interfaces.
- Code action to implement missing methods from directly extended abstract
  classes.
- PHPDoc creation for function and method declarations with parameter and
  return tags, plus direct `throw new ExceptionClass(...)` throws tags.
- Local PHPDoc `@var Type $variable` annotations for instance method
  resolution.
- Inline local PHPDoc `@var Type` annotations for the following assignment.
- Function-like PHPDoc `@param Type $variable` annotations for instance method
  resolution.
- PHPDoc `@mixin ClassName` annotations for class instance method resolution.
- PHPDoc `@method` annotations for class magic method resolution.
- Namespaced same-file functions.
- Static methods and constructors when the class is indexed, including class
  names imported with normal, grouped, or aliased `use` declarations.
- Function calls imported with `use function`.
- Instance methods when the receiver type is locally obvious from a typed
  parameter or `$var = new ClassName(...)`.
- Instance methods when the receiver type flows through a simple local variable
  alias.
- Instance methods when the receiver type comes from a resolved call with a
  class return type.
- Instance methods for native `self`, `static`, and direct `parent` parameter
  receiver types.
- Project symbols under Composer `autoload` and `autoload-dev` `psr-4`,
  `classmap`, and `files` entries.
- A seed set of PHP internal functions, such as `str_replace`, `json_encode`,
  `preg_match`, `in_array`, `str_starts_with`, and `array_values`, with PHP
  manual links in hover.
- Calls that already contain safe named arguments and still have remaining
  positional arguments. Rephactor inserts only the missing names.
- Projects without a Composer PHP version constraint, or projects whose
  `require.php` constraint requires PHP 8 or newer.

## Unsupported Cases

Rephactor returns no action instead of guessing for:

- dynamic calls such as `$fn(...)` or `$object->$method(...)`
- calls with unpacking (`...$args`)
- function imports and grouped-import rewrites
- calls whose existing named arguments do not match the resolved signature
- ambiguous symbols
- unknown parameter names
- PHP internal functions outside the seeded stub set
- completion for dynamic receivers or unresolved classes
- PSR-12 structural formatting beyond trailing-whitespace cleanup
- static-analysis diagnostics beyond parser, callable-resolution, and
  conservative return/argument/assignment type mismatch errors
- Composer autoload modes other than `psr-4`, `classmap`, and `files` entries
  in `autoload` or `autoload-dev`
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
