use std::error::Error;

use git2::{
    self, Branch, BranchType, Cred, FetchOptions, FetchPrune, ObjectType, Oid, Remote,
    RemoteCallbacks, Repository,
};

use crate::err::GitError;

pub struct Git {
    repo: Repository,
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

fn strip_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.starts_with(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
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
    pub fn new(repo: Repository) -> Self {
        Git { repo }
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
            strip_prefix(default_ref, "refs/heads/").unwrap_or(default_ref)
        );
        for result in self.repo.branches(Some(BranchType::Local))? {
            let (branch, _) = result?;
            if let Ok(upstream) = branch.upstream() {
                if ostr!(upstream.name()?) == default_name {
                    return Ok((upstream, Some(branch)));
                }
            }
        }
        let r = self.repo.find_reference(&default_ref)?;
        Ok((Branch::wrap(r), None))
    }

    pub fn fetch(&self, remote: &mut Remote) -> Result<(), Box<dyn Error>> {
        let fetch_refspecs = remote.fetch_refspecs()?;
        let mut refspecs = vec![];
        for refspec in fetch_refspecs.iter() {
            refspecs.push(ostr!(refspec));
        }
        let mut remote_callbacks = RemoteCallbacks::new();
        remote_callbacks.credentials(|_url, username_from_url, _allowed_types| {
            Cred::ssh_key_from_agent(username_from_url.unwrap())
        });

        let remote_name = if let Some(ref name) = remote.name() {
            name.to_string() + "/"
        } else {
            "".to_string()
        };
        remote_callbacks.update_tips(move |s, from, to| {
            let remote_ref = strip_prefix(s, "refs/remotes/").unwrap_or(s);
            let branch = strip_prefix(remote_ref, &remote_name).unwrap_or(remote_ref);
            if from.is_zero() {
                println!(" * [new branch]            {:14} -> {}", branch, remote_ref);
            } else if to.is_zero() {
                println!(
                    " - [deleted]               {:14} -> {}",
                    "(none)", remote_ref
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
                        from, to, branch, remote_ref
                    );
                } else {
                    println!(
                        " + {:.10}..{:.10}  {:14} -> {:14} (forced update)",
                        from, to, branch, remote_ref
                    );
                }
            }
            true
        });
        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(remote_callbacks);
        fetch_options.prune(FetchPrune::On);
        Ok(remote.fetch(&refspecs, Some(&mut fetch_options), None)?)
    }

    pub fn local_branches(&self) -> Result<Vec<Branch>, Box<dyn Error>> {
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

    pub fn update_ref(
        &self,
        branch: &mut Branch,
        remote_branch: &Branch,
    ) -> Result<(), Box<dyn Error>> {
        let rc = self
            .repo
            .reference_to_annotated_commit(remote_branch.get())?;
        let msg = format!(
            "update-ref: moving from {} to {}",
            ostr!(branch.name()?),
            ostr!(remote_branch.name()?)
        );
        branch.get_mut().set_target(rc.id(), &msg)?;
        Ok(())
    }

    pub fn remote(&self, branch: &Branch) -> Result<Remote<'_>, Box<dyn Error>> {
        let branch_name = ostr!(branch.get().name());
        let name = if let Ok(buf) = self.repo.branch_upstream_remote(branch_name) {
            ostr!(buf.as_str()).to_string()
        } else {
            self.repo.config()?.get_string(&format!(
                "branch.{}.pushremote",
                ostr!(branch.get().shorthand())
            ))?
        };
        Ok(self.repo.find_remote(&name)?)
    }
}

pub fn is_branch_same(b1: &Branch, b2: &Branch) -> Result<bool, Box<dyn Error>> {
    let n1 = b1.name_bytes()?;
    let n2 = b2.name_bytes()?;
    Ok(n1 == n2)
}
