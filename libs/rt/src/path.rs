//! Path names, cleaned lexically (userland/sessions.md, Plan 9's rule): `..` is resolved in the
//! name before any lookup, never by asking a server for a parent, so it cannot climb above the
//! root it is resolved against. Clients clean here; the 9P server skeleton applies the same rules
//! again to every walk.

use alloc::vec::Vec;

/// The most bytes in one path component.
pub const MAX_NAME: usize = 255;
/// The most components in a cleaned path, and the deepest a 9P fid may be below its root.
pub const MAX_COMPONENTS: usize = 64;

/// Why a path was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    /// A component that is not a [`valid_name`].
    BadName,
    /// Deeper than [`MAX_COMPONENTS`].
    TooDeep,
}

/// Whether `name` can be one component: not empty, `.` or `..`, no `/` or NUL, at most
/// [`MAX_NAME`] bytes.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name != "."
        && name != ".."
        && !name.bytes().any(|b| b == b'/' || b == 0)
}

/// The components of `path`, relative to whatever root it is resolved against: empty
/// components and `.` are dropped, `..` removes the component before it and does nothing at the
/// root. A leading `/` changes nothing.
pub fn clean(path: &str) -> Result<Vec<&str>, PathError> {
    let mut out = Vec::new();
    for name in path.split('/') {
        match name {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            name if valid_name(name) => {
                if out.len() == MAX_COMPONENTS {
                    return Err(PathError::TooDeep);
                }
                out.push(name);
            }
            _ => return Err(PathError::BadName),
        }
    }
    Ok(out)
}

/// Whether `path` is already absolute and clean, as a namespace table's names must be: `/`, or
/// `/` followed by valid names joined by single `/`s.
pub fn is_clean_absolute(path: &str) -> bool {
    match path.strip_prefix('/') {
        Some("") => true,
        Some(rest) => rest.split('/').all(valid_name) && rest.split('/').count() <= MAX_COMPONENTS,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_dot_never_climbs_above_the_root() {
        assert_eq!(clean("/a/b/../c"), Ok(alloc::vec!["a", "c"]));
        assert_eq!(clean("../../etc/passwd"), Ok(alloc::vec!["etc", "passwd"]));
        assert_eq!(clean("a/../../.."), Ok(alloc::vec![]));
        assert_eq!(clean("//a/./b//"), Ok(alloc::vec!["a", "b"]));
        assert_eq!(clean(""), Ok(alloc::vec![]));
    }

    #[test]
    fn bad_names_are_refused() {
        assert_eq!(clean("a/b\0c"), Err(PathError::BadName));
        let long = "x".repeat(MAX_NAME + 1);
        assert_eq!(clean(&long), Err(PathError::BadName));
        assert!(valid_name(&long[..MAX_NAME]));
        for bad in ["", ".", "..", "a/b", "a\0"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        let deep = "a/".repeat(MAX_COMPONENTS + 1);
        assert_eq!(clean(&deep), Err(PathError::TooDeep));
        assert_eq!(clean(&"a/".repeat(MAX_COMPONENTS)).map(|c| c.len()), Ok(MAX_COMPONENTS));
    }

    #[test]
    fn clean_absolute_paths() {
        for good in ["/", "/dev/cons", "/net"] {
            assert!(is_clean_absolute(good), "{good:?}");
        }
        for bad in ["", "dev", "/dev/", "//", "/a//b", "/a/../b", "/./a", "/a\0"] {
            assert!(!is_clean_absolute(bad), "{bad:?}");
        }
    }
}
