# Git-HubSync

[![CI](https://github.com/iquiw/git-hubsync/workflows/Rust/badge.svg)](https://github.com/iquiw/git-hubsync/actions)

`git-hubsync` is a clone of `sync` subcommand of [hub](https://hub.github.com/).

So why not to use `hub sync`?  Because **some anti-virus software quarantines it!**

Jokes aside, there are several advantages and disadvantages compared to `hub sync`.
See the features below.

It manages local branches according to corresponding remote branches.

For example, if `topic` branch is checked out and the branch is merged to
`main` branch on the remote `origin`, `git hubsync` does the followings
(and more) in one command.

* Update `main` branch to `origin/main`.
* Checkout `main` branch.
* Delete `topic` branch.

```console
$ git hubsync
current branch: topic
default remote: origin
   7266d84eac..8dddfaba30  main           -> origin/main
 * [new branch]            new            -> origin/new
 - [deleted]               (none)         -> origin/topic
remote default: main

Deleted branch topic (was 495368b)
Updated branch main (was 7266d84)
```

## Features

* It does not use `git` command.
  Thanks [git2](https://github.com/rust-lang/git2-rs) and [git2_credentials](https://github.com/davidB/git2_credentials)!
  So it should be faster than `hub sync`, at least on Windows.
* It selects remote of the current branch, instead of the first remote.
* It can detect default branch of the remote, even if `refs/remotes/<remote>/HEAD`
  does not exist.
* It uses `branch.<branch>.pushremote` as remote if `branch.<branch>.remote`
  is not found. And it uses `remote.pushdefault` if both do not exist.
  Some tools like [Magit](https://magit.vc/) utilize `pushremote` and `remote.pushdefault`.
* If default branch of the remote does not exist locally, choose "main" or
  "master" as default branch and its remote as alternate remote.

## Behavior

### Update

When upstream branch is updated;

* If local branch is the current branch and an ancestor of the upstream,
  fast-forward the local branch.
* If local branch is an ancestor of the upstream, update the reference.
* Otherwise, show warning message.

### Delete

When upstream branch is deleted;

* If local branch is the current branch and merged to default branch of the
  remote, switch to the default branch and delete the local branch.
* If local branch is merged to default branch of the remote, delete the
  local branch.
* Otherwise, show warning message.

## Flow

1. Find current branch.
2. Find corresponding remote (main remote) of the current branch.
3. Fetch from the main remote.
4. Detect default branch of the remote.
5. For each local branch,
   1. Skip if remote of the branch is the same as main remote or alternate remote.
   2. Skip if the upstream branch is same as the branch.
   3. Update the branch if it is an ancestor of the upstream branch.
   4. If the upstream branch is deleted and the local branch is merged
      to the default branch,
      1. If the branch is current,
         1. Skip if the default branch does not exist.
         2. Switch current branch to the default branch.
      2. Delete the branch.

## Caveat

* If remote name or branch name is not valid UTF-8, the program aborts.
* It does not support Git LFS.
