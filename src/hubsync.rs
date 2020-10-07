use std::error::Error;

use git2::{self, ErrorClass, ErrorCode, Repository};

use crate::err::GitError;
use crate::git::{self, Git};

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
        match branch.upstream() {
            Ok(upstream) => {
                let range = git.new_range(&branch, &upstream)?;
                if range.is_identical() {
                    continue;
                } else if range.is_ancestor()? {
                    if git::is_branch_same(&branch, &current_branch)? {
                        git.merge(&mut branch, &upstream)?;
                    } else {
                        git.update_ref(&mut branch, &upstream)?;
                    }
                    println!(
                        "Updated branch {} (was {:.7})",
                        ostr!(branch.name()?),
                        range.beg_oid()
                    );
                } else {
                    println!(
                        "warning: '{}' seems to contain unpushed commits",
                        ostr!(branch.name()?)
                    );
                }
            }
            Err(e) => {
                if e.class() == ErrorClass::Reference && e.code() == ErrorCode::NotFound {
                    let range = git.new_range(&branch, &remote_default_branch)?;
                    if range.is_ancestor()? {
                        if git::is_branch_same(&branch, &current_branch)? {
                            let tmp = odefault_branch;
                            odefault_branch = None;
                            if let Some(default_branch) = tmp {
                                git.set_head(&default_branch)?;
                                current_branch = default_branch;
                            } else {
                                odefault_branch = tmp;
                                println!(
                                    "warning: no default branch, skipping to delete '{}'",
                                    ostr!(branch.name()?)
                                );
                                continue;
                            }
                        }
                        branch.delete()?;
                        println!(
                            "Deleted branch {} (was {:.7})",
                            ostr!(branch.name()?),
                            branch.get().peel_to_commit()?.id()
                        );
                    } else {
                        println!(
                            "warning: '{}' was deleted on {}, but appears not merged into '{}'",
                            ostr!(branch.name()?),
                            ostr!(remote.name()),
                            ostr!(remote_default_branch.name()?)
                        );
                    }
                } else if e.class() == ErrorClass::Config && e.code() == ErrorCode::NotFound {
                    // push-remote
                    continue;
                } else {
                    return Err(e.into());
                }
            }
        }
    }
    Ok(())
}
