use std::error::Error;
use std::fmt;

use colored::Colorize;
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

impl fmt::Display for BranchAction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let (tag, upstream) = match self {
            BranchAction::UpToDate => ("up-to-date", None),
            BranchAction::Merge(upstream, _) => ("merge ", upstream.name().unwrap_or(None)),
            BranchAction::UpdateRef(upstream, _) => {
                ("update-ref ", upstream.name().unwrap_or(None))
            }
            BranchAction::Unpushed => ("unpushed", None),
            BranchAction::Unmerged => ("unmerged", None),
            BranchAction::CheckoutAndDelete => ("checkout-and-delete", None),
            BranchAction::Delete => ("delete", None),
            BranchAction::NoDefault => ("nodefault", None),
        };
        write!(f, "{}{}", tag, upstream.unwrap_or(""))
    }
}

pub fn hubsync() -> Result<(), Box<dyn Error>> {
    let repo = Repository::open_from_env()?;
    let config = repo.config()?;
    let git = Git::new(repo, config);
    let mut current_branch = git.current_branch()?;

    println!("current branch: {}", ostr!(current_branch.name()?));
    let mut default_remote = find_default_remote(&git)?;
    println!("default remote: {}", ostr!(default_remote.name()));
    git.fetch(&mut default_remote)?;
    let (remote_default_branch, mut odefault_branch) = git.default_branch(&default_remote)?;
    if let Some(ref default_branch) = odefault_branch {
        println!("remote default: {}", ostr!(default_branch.name()?));
    } else {
        println!("remote default: {}", ostr!(remote_default_branch.name()?));
    }
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
                git.fastforward(&mut branch, &upstream)?;
                println!(
                    "{} {} (was {:.7})",
                    "Updated branch".green(),
                    ostr!(branch.name()?).bright_green(),
                    oid
                );
            }
            BranchAction::UpdateRef(upstream, oid) => {
                git.update_ref(&mut branch, &upstream)?;
                println!(
                    "{} {} (was {:.7})",
                    "Updated branch".green(),
                    ostr!(branch.name()?).bright_green(),
                    oid
                );
            }
            BranchAction::Unpushed => {
                println!(
                    "{}: '{}' seems to contain unpushed commits",
                    "warning".bright_yellow(),
                    ostr!(branch.name()?)
                );
            }
            BranchAction::Unmerged => {
                println!(
                    "{}: '{}' was deleted on {}, but appears not merged into '{}'",
                    "warning".bright_yellow(),
                    ostr!(branch.name()?),
                    ostr!(remote.name()),
                    ostr!(remote_default_branch.name()?)
                );
            }
            BranchAction::CheckoutAndDelete => {
                let tmp = odefault_branch;
                odefault_branch = None;
                if let Some(default_branch) = tmp {
                    git.checkout(&default_branch)?;
                    current_branch = default_branch;
                }
                action_delete(&mut branch)?;
            }
            BranchAction::Delete => {
                action_delete(&mut branch)?;
            }
            BranchAction::NoDefault => {
                println!(
                    "{}: no default branch, skipping to delete '{}'",
                    "warning".bright_yellow(),
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
        "{} {} (was {:.7})",
        "Deleted branch".magenta(),
        ostr!(branch.name()?).bright_magenta(),
        branch.get().peel_to_commit()?.id()
    );
    Ok(())
}

fn find_branch_action<'a>(
    git: &'a Git,
    branch: &Branch<'a>,
    current_branch: &Branch,
    remote_default_branch: &Branch,
    odefault_branch: Option<&Branch<'a>>,
) -> Result<BranchAction<'a>, Box<dyn Error>> {
    match git.upstream(&branch) {
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

fn find_default_remote<'a>(git: &'a Git) -> Result<git2::Remote<'a>, Box<dyn Error>> {
    if let Some(remote) = git.only_one_remote()? {
        Ok(remote)
    } else {
        let branch = git.current_branch()?;
        git.remote(&branch)
    }
}

#[cfg(test)]
mod test {
    use std::env;
    use std::error::Error;
    use std::fs::create_dir;
    use std::process::Command;
    use std::sync::Once;

    use git2::{self, BranchType, Repository};

    use super::{find_branch_action, find_default_remote};
    use crate::git::Git;

    static START: Once = Once::new();

    fn setup_once() {
        START.call_once(|| {
            setup().unwrap();
        });
    }

    fn setup() -> Result<(), Box<dyn Error>> {
        let mut tar_file = env::current_dir()?;
        tar_file.push("ght.tar.gz");

        let mut tmp_dir = env::temp_dir();
        tmp_dir.push("git-hubsync-test");
        if !tmp_dir.is_dir() {
            create_dir(&tmp_dir)?;
        }
        env::set_current_dir(&tmp_dir)?;
        tmp_dir.push("ght");
        if tmp_dir.is_dir() {
            Command::new("rm").args(&["-rf", "ght"]).status()?;
        }
        Command::new("tar").arg("xzf").arg(tar_file).status()?;
        env::set_current_dir(&tmp_dir)?;
        Command::new("git").args(&["fetch", "--prune"]).status()?;
        Ok(())
    }

    fn test_find_branch_action(
        branch_name: &str,
        current: &str,
        odefault: Option<&str>,
    ) -> Result<String, Box<dyn Error>> {
        setup_once();
        Command::new("git").args(&["switch", current]).status()?;

        let repo = Repository::open_from_env()?;
        let branch = repo.find_branch(branch_name, BranchType::Local)?;
        let current_branch = repo.find_branch(current, BranchType::Local)?;
        let remote_default_branch = repo.find_branch("origin/master", BranchType::Remote)?;
        let default_branch = if let Some(default) = odefault {
            Some(repo.find_branch(default, BranchType::Local)?)
        } else {
            None
        };
        let repo = Repository::open_from_env()?;
        let config = repo.config()?;
        let git = Git::new(repo, config);

        let action = find_branch_action(
            &git,
            &branch,
            &current_branch,
            &remote_default_branch,
            default_branch.as_ref(),
        )?;
        Ok(format!("{}", action))
    }

    #[test]
    fn find_branch_action_merge() {
        let action_str = test_find_branch_action("master", "master", None).unwrap();
        assert_eq!(&action_str, "merge origin/master");
    }

    #[test]
    fn find_branch_action_up_to_date() {
        let action_str = test_find_branch_action("up-to-date", "master", None).unwrap();
        assert_eq!(&action_str, "up-to-date");
    }

    #[test]
    fn find_branch_action_update_ref() {
        let action_str = test_find_branch_action("ff", "master", None).unwrap();
        assert_eq!(&action_str, "update-ref origin/ff");
    }

    #[test]
    fn find_branch_action_unpushed() {
        let action_str = test_find_branch_action("non-ff", "master", None).unwrap();
        assert_eq!(&action_str, "unpushed");
    }

    #[test]
    fn find_branch_action_delete() {
        let action_str = test_find_branch_action("deleted", "master", None).unwrap();
        assert_eq!(&action_str, "delete");
    }

    #[test]
    fn find_branch_action_nodefault() {
        let action_str = test_find_branch_action("deleted", "deleted", None).unwrap();
        assert_eq!(&action_str, "nodefault");
    }

    #[test]
    fn find_branch_action_checkout_and_delete() {
        let action_str = test_find_branch_action("deleted", "deleted", Some("master")).unwrap();
        assert_eq!(&action_str, "checkout-and-delete");
    }

    #[test]
    fn find_branch_action_unmerged() {
        let action_str = test_find_branch_action("unmerge-deleted", "master", None).unwrap();
        assert_eq!(&action_str, "unmerged");
    }

    static START2: Once = Once::new();

    fn setup2_once() {
        START2.call_once(|| {
            setup2().unwrap();
        });
    }

    fn setup2() -> Result<(), Box<dyn Error>> {
        let mut tmp_dir = env::temp_dir();
        tmp_dir.push("git-hubsync-test");
        env::set_current_dir(&tmp_dir)?;
        tmp_dir.push("ght2");
        if tmp_dir.is_dir() {
            Command::new("rm").args(&["-rf", "ght2"]).status()?;
        }
        Command::new("git")
            .args(&[
                "clone",
                "https://github.com/iquiw/git-hubsync-test2.git",
                "ght2",
            ])
            .status()?;
        env::set_current_dir(&tmp_dir)?;
        Ok(())
    }

    #[test]
    fn find_default_remote_upstream() {
        setup2_once();

        let repo = Repository::open_from_env().unwrap();
        let config = repo.config().unwrap();
        let git = Git::new(repo, config);
        let remote = find_default_remote(&git).unwrap();
        assert_eq!(remote.name().unwrap(), "origin");
    }

    #[test]
    fn find_default_remote_no_upstream() {
        setup2_once();
        Command::new("git")
            .args(&["switch", "-c", "test"])
            .status()
            .unwrap();

        let repo = Repository::open_from_env().unwrap();
        let config = repo.config().unwrap();
        let git = Git::new(repo, config);
        let remote = find_default_remote(&git).unwrap();
        assert_eq!(remote.name().unwrap(), "origin");
    }
}
