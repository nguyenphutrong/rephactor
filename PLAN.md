# PLAN

## Assumptions

- The server is a companion LSP, not a replacement for Intelephense or Phpactor.
- The first editor target is Zed.
- The first useful feature is a manual code action named `Add names to arguments`.
- Correctness is more important than broad coverage. Unsafe cases should be
  skipped instead of guessed.
- PHP 8.0 or newer is required because named arguments are a PHP 8 feature.

## Product Contract

When the cursor or selection is inside a supported PHP call expression, the
server may offer a code action that converts positional arguments to named
arguments by inserting parameter identifiers.

The action must not be offered when:

- the callable cannot be resolved uniquely
- the call contains unpacking (`...`)
- the call target is dynamic, such as `$fn(...)` or `$object->$method(...)`
- the conversion would reorder arguments
- existing named arguments do not match the resolved parameter order
- parameter names are unavailable or synthetic
- the file or project PHP version is below 8.0

If a call already has safe named arguments and still has remaining positional
arguments, the action may insert only the missing names.

## Technical Approach

1. Implement an LSP stdio server with `tower-lsp`.
2. Parse PHP files with `tree-sitter-php`.
3. Track open document text from `didOpen` and `didChange`.
4. Locate the smallest call expression containing the requested range.
5. Build a project index for functions, classes, methods, constructors, traits,
   and namespaces.
6. Resolve the call target to a single callable signature.
7. Generate non-overlapping text edits that insert `name: ` before positional
   arguments.
8. Return a `WorkspaceEdit` through `CodeAction`.

## MVP Milestone

The MVP is intentionally conservative:

- LSP initializes successfully over stdio.
- Server advertises code action support.
- `textDocument/codeAction` returns an empty list until parser plumbing is in
  place.
- Unit tests cover LSP capability construction and document state updates.

## First Real Conversion Milestone

Support same-file functions only:

```php
function send_invoice($invoice, $notify) {}

send_invoice($invoice, true);
```

Expected output:

```php
send_invoice(invoice: $invoice, notify: true);
```

Verification:

- unit tests for parsing and call lookup
- unit tests for edit generation
- fixture test that applies edits to source text
- manual Zed code-action check

## Risk Controls

- Prefer no action over a risky action.
- Keep conversion logic pure and fixture-tested.
- Keep LSP transport thin; it should call tested semantic functions.
- Do not add formatter-like rewriting. Insert only required prefixes.
- Preserve comments, whitespace, and argument expressions exactly.

## Open Questions

- Whether to use Composer's generated autoload data directly or implement
  minimal PSR-4 scanning first.
- Whether to vendor PHP internal stubs or consume an existing package.
- Whether Zed extension packaging should install the binary or rely on a local
  command path during early development.
- Whether Zed should grow a core code-action edit preview, since LSP does not
  provide a separate preview payload and Rephactor can only return
  `WorkspaceEdit`.
