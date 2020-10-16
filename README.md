# Git-HubSync

[![CI](https://github.com/iquiw/git-hubsync/workflows/Rust/badge.svg)](https://github.com/iquiw/git-hubsync/actions)

`git-hubsync` is a clone of `sync` subcommand of [hub](https://hub.github.com/).

So why not to use `hub sync`?  Because **some anti-virus software quarantines it!**

## Features

* It does not use `git` command.  Thanks [git2](https://github.com/rust-lang/git2-rs) and [git2_credentials](https://github.com/davidB/git2_credentials)!
  So it should be faster than `hub sync`, at least on Windows.
* It selects remote of the current branch, instead of the first remote.
* It can select default branch of the remote, even if `refs/remotes/<remote>/HEAD` does not exist.
* It uses `branch.<branch>.pushremote` as remote if `branch.<branch>.remote` is not found.
  Some tools like [Magit](https://magit.vc/) utilize `pushremote`.

## Flow

1. Find current branch.
2. Find corresponding remote (main remote) of the current branch.
3. Fetch from the main remote.
4. Find default branch of the remote.
5. For each local branch,
   1. Skip if remote of the branch is the same as main remote.
   2. Skip if the remote branch is same as the branch.
   3. Update the branch if it is ancestor of the remote branch.
   4. If the remote branch is deleted,
      1. If the branch is current,
         1. Skip if the default branch does not exist.
         2. Switch current branch to the default branch.
      2. Delete the branch.


## Caveat

* If remote name or branch name is not valid UTF-8, the program aborts.
