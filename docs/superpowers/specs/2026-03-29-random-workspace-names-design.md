# Random Workspace Names

Auto-generate `adjective-animal` workspace names when the user doesn't provide one, similar to Docker's random container naming but animal-themed.

## CLI Change

Rename the `add` subcommand to `new`. The `name` positional argument becomes an optional `--name` flag.

```
# Before
jjws add my-feature

# After
jjws new                  # generates e.g. "bold-otter"
jjws new --name my-feature  # explicit name
```

## Name Generator (`src/names.rs`)

Two static arrays of ~50 entries each: adjectives and animals.

```rust
pub fn generate(exists: impl Fn(&str) -> bool) -> String
```

- Derive an index from `SystemTime::now()` duration since epoch, using nanoseconds.
- Use different arithmetic (e.g. nanos % len for adjective, nanos / len % len for animal) to reduce correlation.
- Format as `{adjective}-{animal}`.
- If `exists` returns true for the base name, append an incrementing integer starting at 2: `bold-otter2`, `bold-otter3`, etc.
- Return the first non-colliding name.

~50 x ~50 = ~2500 combinations before number suffixes are needed.

## Code Changes

### `src/main.rs`

- Rename `Command::Add` to `Command::New`.
- Change `name: String` to `#[arg(long)] name: Option<String>`.
- Rename match arm, pass `Option<String>` through.

### `src/lib.rs`

- Rename `AddOptions` to `NewOptions`, `add()` to `new_workspace()`.
- `NewOptions.name` becomes `Option<String>`.
- When `None`, call `names::generate()` with a closure checking existing workspace names via `current.repo.view().wc_commit_ids()`.
- Print the generated name in the output.

### `completions/jjws.fish`

- Replace `add` with `new` in subcommand completions.
- Remove `--no-tab` completion for `add`, add it under `new`.

### `README.md`

- Update usage block and feature list to reflect `new` and auto-naming.

## Collision Check

The `exists` closure checks against jj's known workspace names (from the repo view), not the filesystem. This is consistent with how `create_workspace` validates uniqueness.
