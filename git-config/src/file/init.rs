use std::path::Path;

use crate::{
    file::{from_paths, resolve_includes},
    parse, File,
};

impl File<'static> {
    /// Constructs a `git-config` file from the provided path.
    ///
    /// # Errors
    ///
    /// Returns an error if there was an IO error or if the file wasn't a valid
    /// git-config file.
    pub fn at<P: AsRef<Path>>(path: P) -> Result<Self, parse::events::from_path::Error> {
        parse::Events::from_path(path).map(Self::from)
    }

    /// Constructs a `git-config` file from the provided paths in the order provided.
    /// This is neither zero-copy nor zero-alloc.
    ///
    /// # Errors
    ///
    /// Returns an error if there was an IO error or if a file wasn't a valid
    /// git-config file.
    ///
    /// [`git-config`'s documentation]: https://git-scm.com/docs/git-config#Documentation/git-config.txt-FILES
    pub fn from_paths(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        options: from_paths::Options<'_>,
    ) -> Result<Self, from_paths::Error> {
        let mut target = Self::new();
        for path in paths {
            let path = path.as_ref();
            let mut config = Self::at(path)?;
            resolve_includes(&mut config, Some(path), options)?;
            target.append(config);
        }
        Ok(target)
    }
}

impl<'a> File<'a> {
    /// Constructs an empty `git-config` file.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}