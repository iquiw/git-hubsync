use std::error::Error;

use git2::{
    self, Branch, BranchType, Config, FetchOptions, FetchPrune, ObjectType, Oid, Remote,
    RemoteCallbacks, Repository,
};
use git2_credentials::CredentialHandler;

use crate::err::GitError;

pub struct Git {
    repo: Repository,
    config: Config,
}

macro_rules! ostr {
    ($expr:expr) => {
        match $expr {
            Some(s) => s,
            None => {
                return Err(GitError::new(format!("Unable to convert to string")).into());
            }
        }
    };
}

fn prefix_stripped<'a>(s: &'a str, prefix: &str) -> &'a str {
    if let Some(stripped) = s.strip_prefix(prefix) {
        stripped
    } else {
        s
    }
}

pub struct Range<'a> {
    repo: &'a Repository,
    beg: Oid,
    end: Oid,
}

impl Range<'_> {
    pub fn beg_oid(&self) -> Oid {
        self.beg
    }

    pub fn is_identical(&self) -> bool {
        self.beg == self.end
    }

    pub fn is_ancestor(&self) -> Result<bool, Box<dyn Error>> {
        Ok(self.repo.graph_descendant_of(self.end, self.beg)?)
    }
}

impl Git {
    pub fn new(repo: Repository, config: Config) -> Self {
        Git { repo, config }
    }

    pub fn checkout(&self, branch: &Branch) -> Result<(), Box<dyn Error>> {
        self.repo
            .checkout_tree(&branch.get().peel(ObjectType::Commit)?, None)?;
        self.repo.set_head(ostr!(branch.get().name()))?;
        Ok(())
    }

    pub fn current_branch(&self) -> Result<Branch<'_>, Box<dyn Error>> {
        if self.repo.head_detached()? {
            Err(GitError::new("Head is detached".to_string()).into())
        } else {
            Ok(Branch::wrap(self.repo.head()?))
        }
    }

    pub fn default_branch(
        &self,
        remote: &Remote,
    ) -> Result<(Branch<'_>, Option<Branch<'_>>), Box<dyn Error>> {
        let buf = remote.default_branch()?;
        let default_ref = ostr!(buf.as_str());
        let default_name = format!(
            "{}/{}",
            ostr!(remote.name()),
            prefix_stripped(default_ref, "refs/heads/")
        );
        for result in self.repo.branches(Some(BranchType::Local))? {
            let (branch, _) = result?;
            if let Ok(upstream) = branch.upstream() {
                if ostr!(upstream.name()?) == default_name {
                    return Ok((upstream, Some(branch)));
                }
            }
        }
        let upstream = self.repo.find_branch(&default_name, BranchType::Remote)?;
        Ok((upstream, None))
    }

    pub fn branch_and_remote(
        &self,
        name: &str,
    ) -> Result<(Branch<'_>, Remote<'_>), Box<dyn Error>> {
        let branch = self.repo.find_branch(name, BranchType::Local)?;
        let remote = self.remote(&branch)?;
        Ok((branch, remote))
    }

    pub fn update_tips(
        &self,
        remote: &Remote,
        s: &str,
        from: Oid,
        to: Oid,
    ) -> Result<(), Box<dyn Error>> {
        if to.is_zero() {
            println!(
                " - [deleted]               {:14} -> {}",
                "(none)",
                prefix_stripped(s, "refs/remotes/")
            );
            return Ok(());
        }
        let refer = self.repo.find_reference(s)?;
        let (mark, from_name, to_name) = if refer.is_tag() {
            let name = ostr!(refer.shorthand());
            ("tag", name.to_string(), name.to_string())
        } else {
            let mut result = ("ref", s.to_string(), s.to_string());
            for refspec in remote.refspecs() {
                if let Ok(src) = refspec.rtransform(s) {
                    if refer.is_remote() {
                        result = (
                            "branch",
                            prefix_stripped(ostr!(src.as_str()), "refs/heads/").to_string(),
                            ostr!(refer.shorthand()).to_string(),
                        );
                    } else {
                        result = ("ref", ostr!(src.as_str()).to_string(), s.to_string());
                    }
                    break;
                }
            }
            result
        };
        if from.is_zero() {
            println!(
                " * {:24}{:14} -> {}",
                format!("[new {}]", mark),
                from_name,
                to_name
            );
        } else {
            let range = Range {
                repo: &self.repo,
                beg: from,
                end: to,
            };
            if range.is_ancestor().unwrap_or(false) {
                println!(
                    "   {:.10}..{:.10}  {:14} -> {:14}",
                    from, to, from_name, to_name
                );
            } else {
                println!(
                    " + {:.10}..{:.10}  {:14} -> {:14} (forced update)",
                    from, to, from_name, to_name
                );
            }
        }
        Ok(())
    }

    pub fn fetch(&self, remote: &mut Remote) -> Result<(), Box<dyn Error>> {
        let fetch_refspecs = remote.fetch_refspecs()?;
        let mut refspecs = vec![];
        for refspec in fetch_refspecs.iter() {
            refspecs.push(ostr!(refspec));
        }
        let mut remote_callbacks = RemoteCallbacks::new();
        let config = self.repo.config()?;
        let mut ch = CredentialHandler::new(config);
        remote_callbacks.credentials(move |url, username_from_url, allowed_types| {
            ch.try_next_credential(url, username_from_url, allowed_types)
        });

        let remote_clone = remote.clone();
        remote_callbacks.update_tips(move |s, from, to| {
            if let Err(e) = self.update_tips(&remote_clone, s, from, to) {
                println!("s: {}", e);
            }
            true
        });
        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(remote_callbacks);
        fetch_options.prune(FetchPrune::On);
        Ok(remote.fetch(&refspecs, Some(&mut fetch_options), None)?)
    }

    pub fn local_branches(&self) -> Result<Vec<Branch<'_>>, Box<dyn Error>> {
        let mut v = vec![];
        for result in self.repo.branches(Some(BranchType::Local))? {
            let (branch, _) = result?;
            v.push(branch)
        }
        Ok(v)
    }

    pub fn fastforward(
        &self,
        branch: &mut Branch,
        upstream: &Branch,
    ) -> Result<(), Box<dyn Error>> {
        self.repo
            .checkout_tree(&upstream.get().peel(ObjectType::Commit)?, None)?;
        self.update_ref(branch, upstream)?;
        Ok(())
    }

    pub fn new_range(
        &self,
        local: &Branch,
        upstream: &Branch,
    ) -> Result<Range<'_>, Box<dyn Error>> {
        Ok(Range {
            repo: &self.repo,
            beg: local.get().peel_to_commit()?.id(),
            end: upstream.get().peel_to_commit()?.id(),
        })
    }

    pub fn update_ref<'a>(
        &self,
        branch: &mut Branch<'a>,
        remote_branch: &Branch,
    ) -> Result<Branch<'a>, Box<dyn Error>> {
        let rc = self
            .repo
            .reference_to_annotated_commit(remote_branch.get())?;
        let msg = format!(
            "update-ref: moving from {} to {}",
            ostr!(branch.name()?),
            ostr!(remote_branch.name()?)
        );
        let refer = branch.get_mut().set_target(rc.id(), &msg)?;
        Ok(Branch::wrap(refer))
    }

    pub fn only_one_remote(&self) -> Result<Option<Remote<'_>>, Box<dyn Error>> {
        let remotes = self.repo.remotes()?;
        if remotes.len() == 1 {
            if let Some(oremote_name) = remotes.iter().next() {
                let remote_name = ostr!(oremote_name);
                return Ok(Some(self.repo.find_remote(remote_name)?));
            }
        }
        Ok(None)
    }

    pub fn remote(&self, branch: &Branch) -> Result<Remote<'_>, Box<dyn Error>> {
        let branch_name = ostr!(branch.get().name());
        let name = if let Ok(buf) = self.repo.branch_upstream_remote(branch_name) {
            ostr!(buf.as_str()).to_string()
        } else if let Ok(name) = self.config.get_string(&format!(
            "branch.{}.pushremote",
            ostr!(branch.get().shorthand())
        )) {
            name
        } else {
            self.config.get_string("remote.pushdefault")?
        };
        Ok(self.repo.find_remote(&name)?)
    }

    pub fn upstream<'a>(&'a self, branch: &Branch<'a>) -> Result<Branch<'a>, git2::Error> {
        if let Ok(upstream) = branch.upstream() {
            Ok(upstream)
        } else {
            let branch_name = branch.get().shorthand().ok_or_else(|| {
                git2::Error::new(
                    git2::ErrorCode::NotFound,
                    git2::ErrorClass::Invalid,
                    "Unable to get branch name",
                )
            })?;
            let remote_name = self
                .config
                .get_string(&format!("branch.{}.pushremote", branch_name))?;
            Ok(self.repo.find_branch(
                &format!("{}/{}", remote_name, branch_name),
                BranchType::Remote,
            )?)
        }
    }
}

pub fn is_branch_same(b1: &Branch, b2: &Branch) -> Result<bool, Box<dyn Error>> {
    let n1 = b1.name_bytes()?;
    let n2 = b2.name_bytes()?;
    Ok(n1 == n2)
}
