use std::error::Error;

use git2::{self, Branch, ErrorClass, ErrorCode, Oid, Repository};

use crate::err::GitError;
use crate::git::{self, Git};

enum BranchAction<'a> {
    UpToDate,
    Merge(Branch<'a>, Oid),
    UpdateRef(Branch<'a>, Oid),
    Unpushed,
    CheckoutAndDelete,
    NoDefault,
    Delete,
    Unmerged,
}

pub fn hubsync() -> Result<(), Box<dyn Error>> {
    let repo = Repository::open_from_env()?;
    let git = Git::new(repo);
    let mut current_branch = git.current_branch()?;

    println!("current branch: {}", ostr!(current_branch.name()?));
    let mut default_remote = git.remote(&current_branch)?;
    println!("default remote: {}", ostr!(default_remote.name()));
    git.fetch(&mut default_remote)?;
    let (remote_default_branch, mut odefault_branch) = git.default_branch(&default_remote)?;
    println!("remote default: {}", ostr!(remote_default_branch.name()?));
    println!();

    for mut branch in git.local_branches()? {
        let remote = match git.remote(&branch) {
            Ok(remote) => remote,
            Err(e) => match e.downcast::<git2::Error>() {
                Ok(ge) => {
                    if ge.class() == ErrorClass::Config && ge.code() == ErrorCode::NotFound {
                        continue;
                    } else {
                        return Err(ge);
                    }
                }
                Err(e) => return Err(e),
            },
        };
        if remote.name() != default_remote.name() {
            continue;
        }
        let action = find_branch_action(
            &git,
            &branch,
            &current_branch,
            &remote_default_branch,
            odefault_branch.as_ref(),
        )?;
        match action {
            BranchAction::UpToDate => { /* no action */ }
            BranchAction::Merge(upstream, oid) => {
                git.merge(&mut branch, &upstream)?;
                println!("Updated branch {} (was {:.7})", ostr!(branch.name()?), oid);
            }
            BranchAction::UpdateRef(upstream, oid) => {
                git.update_ref(&mut branch, &upstream)?;
                println!("Updated branch {} (was {:.7})", ostr!(branch.name()?), oid);
            }
            BranchAction::Unpushed => {
                println!(
                    "warning: '{}' seems to contain unpushed commits",
                    ostr!(branch.name()?)
                );
            }
            BranchAction::Unmerged => {
                println!(
                    "warning: '{}' was deleted on {}, but appears not merged into '{}'",
                    ostr!(branch.name()?),
                    ostr!(remote.name()),
                    ostr!(remote_default_branch.name()?)
                );
            }
            BranchAction::CheckoutAndDelete => {
                let tmp = odefault_branch;
                odefault_branch = None;
                if let Some(default_branch) = tmp {
                    git.set_head(&default_branch)?;
                    current_branch = default_branch;
                }
                action_delete(&mut branch)?;
            }
            BranchAction::Delete => {
                action_delete(&mut branch)?;
            }
            BranchAction::NoDefault => {
                println!(
                    "warning: no default branch, skipping to delete '{}'",
                    ostr!(branch.name()?)
                );
            }
        }
    }
    Ok(())
}

fn action_delete(branch: &mut Branch) -> Result<(), Box<dyn Error>> {
    branch.delete()?;
    println!(
        "Deleted branch {} (was {:.7})",
        ostr!(branch.name()?),
        branch.get().peel_to_commit()?.id()
    );
    Ok(())
}

fn find_branch_action<'a>(
    git: &Git,
    branch: &Branch<'a>,
    current_branch: &Branch,
    remote_default_branch: &Branch,
    odefault_branch: Option<&Branch<'a>>,
) -> Result<BranchAction<'a>, Box<dyn Error>> {
    match branch.upstream() {
        Ok(upstream) => {
            let range = git.new_range(&branch, &upstream)?;
            if range.is_identical() {
                Ok(BranchAction::UpToDate)
            } else if range.is_ancestor()? {
                if git::is_branch_same(&branch, &current_branch)? {
                    Ok(BranchAction::Merge(upstream, range.beg_oid()))
                } else {
                    Ok(BranchAction::UpdateRef(upstream, range.beg_oid()))
                }
            } else {
                Ok(BranchAction::Unpushed)
            }
        }
        Err(e) => {
            if e.class() == ErrorClass::Reference && e.code() == ErrorCode::NotFound
                || /* pushremote */ e.class() == ErrorClass::Config && e.code() == ErrorCode::NotFound
            {
                let range = git.new_range(&branch, &remote_default_branch)?;
                if range.is_ancestor()? {
                    if git::is_branch_same(&branch, &current_branch)? {
                        if odefault_branch.is_some() {
                            Ok(BranchAction::CheckoutAndDelete)
                        } else {
                            Ok(BranchAction::NoDefault)
                        }
                    } else {
                        Ok(BranchAction::Delete)
                    }
                } else {
                    Ok(BranchAction::Unmerged)
                }
            } else {
                Err(e.into())
            }
        }
    }
}
