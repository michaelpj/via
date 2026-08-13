# via

Issue commands across multiple interactive CLI sessions.

## Overview

`via` lets you start named interactive sessions (REPLs, shells, etc.) and control them from any terminal. It's mainly just a wrapper for [teetty](https://github.com/mitsuhiko/teetty) but adds some conveniences making interactions with a command prompt more robust and convenient.


## Disclaimer

This is tool is primarily for my own personal use with LLM coding assistants like Claude and Codex. I do not expect to spend much time maintaining or upgrading `via`, but if someone wants to volunteer to take over, feel free to drop me a message.

Also, I'm not a Rust developer and leaned heavily on Claude to port my original shell script. I've mainly selected Rust because `teetty` is coded in Rust and I didn't want to add any dependencies on top of that tool.

## Example

Start a Nix REPL session in one terminal:
```bash
$ via nix run --delim 'nix-repl>' -- nix repl
...
```

Start a python session in another terminal:
```bash
$ via python run --delim '>>>' -- python
...
```

Interact with both from a third terminal:
```bash
# Check what sessions are running
$ via
Session  Status  Prompt Line  Working Directory  Command
nix      live    nix-repl>    /home/me/projects  nix repl
python   live    >>>          /home/me/projects  python

# Send a command and get the result (delimiter is stored)
$ via nix '1 + 1'
nix-repl> 1 + 1
2

# Pipe commands into the repl
$ echo 'print("hello")' | via python
>>> print("hello")
hello
```

This enables scripting interactive CLIs that weren't designed for automation.

## Installation

Only nix based installation is supported for right now and I assume you are familiar enough with it to not need help.

Assuming you have nix flakes enabled you can try it out with:

```
$ nix shell github:rehno-lindeque/via
$ via --help
```


## Usage

```
via [--simple]                                        # list sessions
via help                                              # show help
via run [--delim 'PROMPT>'] [--bg] -- <cmd> ...       # start session with auto-generated name
via <session> run [--delim 'PROMPT>'] [--bg] -- <cmd> # start a named session
via <session> wait [--until 'PROMPT>']                # wait for prompt (default: stored delim)
via <session> [--delim 'PROMPT>'] [--timeout N] line  # write input and stream until delim
```

`<session>` may be `.`, which resolves to the name of the current directory. This
is handy with git worktrees: from each worktree, `via . <command>` talks to a
session named after that worktree, with no per-worktree name juggling.

```bash
# From a worktree checked out at /path/to/feature-x:
$ via . run --bg --delim 'ghci>' -- cabal repl   # session "feature-x"
$ via . :reload                                  # reload that worktree's repl
$ via . write ':quit'                            # tear it down
```

Low-level commands:

```
via <session> write [line...]                         # write to session stdin
via <session> tail -n N                               # tail last N lines
via <session> tail -f [-n N]                          # follow output in real-time
via <session> tail --since ['PROMPT>']                # tail since last prompt (bare = stored delim)
via <session> tail --delim ['PROMPT>']                # last stanza (bare = stored delim)
via <session> tail --until ['PROMPT>'] [--timeout N]  # stream until prompt (bare = stored delim)
via <session> path                                    # show session directory path
```

## License

Apache-2.0 - See LICENSE file for details
