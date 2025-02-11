## Scripting for Jujutsu
Authors: [Philip Metzger](mailto:philipmetger@bluewin.ch),
  [Austin Seipp](mailto:aseipp@pobox.com)
<!-- TODO: Convert this to a design doc -->
This gives an overview of a maybe planned scripting feature for Jujutsu.

My rough estimate for this project is 2-3 SWE-years for two capable SWEs.

The goal for now is to have something representable for `jjcon` in
Mountain View. An ideal PoC would still be `jj paralelize`.

Intended PoC:

```python

def main(ctx: JujutsuContext):
  repo = ctx.repo
  commit = repo.tip
  author = commit.author
  domain = author.domain
  local = author.local
  ctx.print(f"Last commit on {repo.name} ({commit.change_id)} is from \
   "{author.fullname()}")
```

The shiny future:
```python
# # evolve.mhs
# load("//prelude/paths.bzl", "path")
# load("@builtin", "shell")
# def _copy_file(path: Path, shell: Shell):
#   shell.copy(from=path, to="some_dir")

def _main(ctx: jj.Context):
    if ctx.cli_args.some_thing:
      ctx.output.print("something was set")
    repo = ctx.repo
    if isinstance(repo, PiperRepo):
      ctx.output.print("operating in Google3")
    if isinstance(repo, GitRepo):
      ctx.output.print("operating in OSS")
    git_repo = cast(repo, GitRepo)
    transaction = ctx.start_transaction()
    # ...
    transaction.end(f"did some thing")

mahou_main(
  impl = _main,
  cli_args: {
    "revisions": cli_args.list(type = [Revision], doc = "some doc"),
    "some_thing": cli_args.bool(default = true, doc = "some thing", required = false),
  })
```

### Why?

Git, the currently dominant version control system allows users to run custom
scripts or executables by prefixing them with `git-` so a custom Git absorb
implementation is downloaded as `git-absorb` and then `git absorb` searches
the users `$PATH` finding `git-absorb` and forwarding the args to it. Another
example of such a system in the Rust ecosystem is `cargo` which copied this
from Git  (TODO: is that claim true?).

Jujutsu currently has not chosen to implement this and currently only allows
the invocation of custom scripts with `jj util exec` which then passes the
arguments to the current shell (usually bash, nu or Powershell). This has
limitations that the scripts only occasionally get checked in as they aren't
portable between different OSes.

By adding a `jj script` command which integrates deeply with Jujutsu, we can
allow users to write portable scripts which then can be checked into a repo
or be kept in `$XDG_HOME/config/jj/scripts` allowing custom extensions to
Jujutsu. Having such a language will also allow server implementations to
build a continous integration (CI) system ontop of it. It also allows the
project to avoid adding a `jj op transaction start`, `jj op transaction end`
command pair since many people scripting ontop of `jj` want that.

Also `jj fzf` could entirely work against a "stable ABI/API" by customizing the
output of the relevants scripts.

### Overlap with other proposals

The scripting feature overlaps both with `jj api`(TODO: issues and ddoc) and
the a nice Rust API although it has a different design space, this is because
the interpreter is controlled by the project and allows more control over user
scripts.

A "nice" Rust API is still behold to Rust's SemVer and Hyrum's Law which will
at a certain point make everyone depend on the observable behavior of the API.
It will certainly also improve the kernel of the scripting language, but that
can also be built with the project.

`jj api` will be a RPC-System which integrates with editors and other external
users which may at some point encompass `jj scripts` interpreter, but its
primary use-case won't be it for a looong time which makes the shared subset
minimal.

### Language choice

The language design choice is constrained to available and production ready
scripting languages on crates.io.

#### Rhai

Wasn't really considered yet, I (Philip) only know that it is slow from
the Orange Site.

#### Lua

Lua is the configuration system used by the Neovim editor, it allows has
multiple typed dialects although they're not available to Rust as they're
usually implemented in C++ which would require a large unsafe shim around
them.

* Used in Neovim
* Popular scripting language also used in video games
* Very extensible with meta-tables
* Linting and Migration rules would have to built themselves

Since Lua has metatables it will allow users to override builtins and more
which makes kernel features instantly dependent on Hyrum's Law.

#### Starlark

Starlark is a constrained implementation of Python, primarily used by the
Bazel and Buck2 build systems, it also is used by a variety of Meta/Google
internal systems which means it has a wide adoption in different use-cases.
Note: `starlark-rs` also contains a huge amount of unsafe code which will
need to be revetted for Google3.

* Already used by `buck2`
* Provides static typing
* Already has a linting engine available which allows painless migrations

Since `starlark-rs` also provides analysis it will allow people to work on the
language kernel and continously deprecate and replace features without harming
users, which is a huge gain to the project which already adopted a similar way
to deal with config, command and argument changes.

### Planned features

This list is currently arbitary, so not sorted by priority

* Baseline Starlark support (loading, types and linting)
* User defined types with Records and Enums
* Error handling (Starlark is heavily lacking in this [inspired by bxl])
* Readonly (frozen) access to user config
* Lazyness in repo lookups since this is async in the library (I have been
  thinking about exposing a Revision as just a CommitId)
* Exposing existing commands with `load("@builtin", "run")`
* `jj script foo -- --arg` recursive path lookup from
  `$XDG_HOME/config/jj/script` to `$repo/script/`
* Exposing transactions (a potential subtransaction feature may be needed
  [also useful for `jj run`])
* HTTP/Git Protocol access (would allow custom `jj gerrit send`-likes)


### Upstreaming Process

- [] Convert this to a design doc
- [] Add the no-op `jj script` command
- [] Implement the language in a separate crate depending on `jj-lib` or i
  deally a `jj-api` crate
