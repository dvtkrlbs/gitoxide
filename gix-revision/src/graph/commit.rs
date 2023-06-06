use smallvec::SmallVec;

use super::LazyCommit;
use crate::graph::{Commit, CommitterTimestamp, Either, Generation};

impl<'graph> LazyCommit<'graph> {
    /// Return an iterator over the parents of this commit.
    pub fn iter_parents(&self) -> Parents<'graph> {
        let backing = match &self.backing {
            Either::Left(buf) => Either::Left(gix_object::CommitRefIter::from_bytes(buf)),
            Either::Right((cache, pos)) => Either::Right((*cache, cache.commit_at(*pos).iter_parents())),
        };
        Parents { backing }
    }

    /// Returns the timestamp at which this commit was created.
    ///
    /// This is the single-most important date for determining recency of commits.
    /// Note that this can only fail if the commit is backed by the object database *and* parsing fails.
    pub fn committer_timestamp(&self) -> Result<CommitterTimestamp, gix_object::decode::Error> {
        Ok(match &self.backing {
            Either::Left(buf) => {
                gix_object::CommitRefIter::from_bytes(buf)
                    .committer()?
                    .time
                    .seconds_since_unix_epoch as CommitterTimestamp
            }
            Either::Right((cache, pos)) => cache.commit_at(*pos).committer_timestamp(),
        })
    }

    /// Returns the generation of the commit if it is backed by a commit graph.
    pub fn generation(&self) -> Option<Generation> {
        match &self.backing {
            Either::Left(_) => None,
            Either::Right((cache, pos)) => cache.commit_at(*pos).generation().into(),
        }
    }

    /// Convert ourselves into an owned version, which effectively detaches us from the underlying graph.
    /// Use `new_data()` to provide the `data` field for the owned `Commit`.
    pub fn to_owned<T>(&self, new_data: impl FnOnce() -> T) -> Result<Commit<T>, to_owned::Error> {
        let data = new_data();
        Ok(match &self.backing {
            Either::Left(buf) => {
                use gix_object::commit::ref_iter::Token;
                let iter = gix_object::CommitRefIter::from_bytes(buf);
                let mut parents = SmallVec::default();
                let mut timestamp = None;
                for token in iter {
                    match token? {
                        Token::Tree { .. } => {}
                        Token::Parent { id } => parents.push(id),
                        Token::Author { .. } => {}
                        Token::Committer { signature } => {
                            timestamp = Some(signature.time.seconds_since_unix_epoch as CommitterTimestamp);
                            break;
                        }
                        _ => {
                            unreachable!(
                                "we break naturally after seeing the committer which is always at the same spot"
                            )
                        }
                    }
                }
                Commit {
                    parents,
                    commit_time: timestamp.unwrap_or_default(),
                    generation: None,
                    data,
                }
            }
            Either::Right((cache, pos)) => {
                let mut parents = SmallVec::default();
                let commit = cache.commit_at(*pos);
                for pos in commit.iter_parents() {
                    let pos = pos?;
                    parents.push(cache.commit_at(pos).id().to_owned());
                }
                Commit {
                    parents,
                    commit_time: commit.committer_timestamp(),
                    generation: Some(commit.generation()),
                    data,
                }
            }
        })
    }
}

/// An iterator over the parents of a commit.
pub struct Parents<'graph> {
    backing: Either<
        gix_object::CommitRefIter<'graph>,
        (
            &'graph gix_commitgraph::Graph,
            gix_commitgraph::file::commit::Parents<'graph>,
        ),
    >,
}

impl<'graph> Iterator for Parents<'graph> {
    type Item = Result<gix_hash::ObjectId, iter_parents::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.backing {
            Either::Left(it) => {
                for token in it {
                    match token {
                        Ok(gix_object::commit::ref_iter::Token::Tree { .. }) => continue,
                        Ok(gix_object::commit::ref_iter::Token::Parent { id }) => return Some(Ok(id)),
                        Ok(_unused_token) => break,
                        Err(err) => return Some(Err(err.into())),
                    }
                }
                None
            }
            Either::Right((cache, it)) => it
                .next()
                .map(|r| r.map(|pos| cache.id_at(pos).to_owned()).map_err(Into::into)),
        }
    }
}

///
pub mod iter_parents {
    /// The error returned by the [`Parents`][super::Parents] iterator.
    #[derive(Debug, thiserror::Error)]
    #[allow(missing_docs)]
    pub enum Error {
        #[error("An error occurred when parsing commit parents")]
        DecodeCommit(#[from] gix_object::decode::Error),
        #[error("An error occurred when parsing parents from the commit graph")]
        DecodeCommitGraph(#[from] gix_commitgraph::file::commit::Error),
    }
}

///
pub mod to_owned {
    /// The error returned by [`to_owned()`][crate::graph::LazyCommit::to_owned()].
    #[derive(Debug, thiserror::Error)]
    #[allow(missing_docs)]
    pub enum Error {
        #[error("A commit could not be decoded during traversal")]
        Decode(#[from] gix_object::decode::Error),
        #[error("Could not find commit position in graph when traversing parents")]
        CommitGraphParent(#[from] gix_commitgraph::file::commit::Error),
    }
}
