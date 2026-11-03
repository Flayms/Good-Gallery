//! Which version is newer, and where a version's release lives (spec 031). The connection itself is `media::update`'s.

use std::fmt;

const RELEASES: &str = "https://github.com/Flayms/Good-Gallery/releases";

/// The latest published release, as GitHub's API tells it.
pub const LATEST_RELEASE_API: &str = "https://api.github.com/repos/Flayms/Good-Gallery/releases/latest";

/// A released version. Compared by major, then minor, then patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// `1.2.3` or `v1.2.3`; nothing else, so a version is safe to put into a URL and a file name.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.').map(|part| {
            let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            digits.then(|| part.parse::<u32>().ok()).flatten()
        });
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
        };
        parts.next().is_none().then_some(version)
    }

    /// The installer's file name (REQ-014-10).
    pub fn installer_name(&self) -> String {
        format!("good-gallery-{self}-windows-x64-setup.exe")
    }

    /// Where the installer can be downloaded.
    pub fn installer_url(&self) -> String {
        format!("{RELEASES}/download/v{self}/{}", self.installer_name())
    }

    /// Where the installer's signature can be downloaded (REQ-031-09).
    pub fn signature_url(&self) -> String {
        format!("{}.sig", self.installer_url())
    }

    /// The release's page, for people.
    pub fn release_url(&self) -> String {
        format!("{RELEASES}/tag/v{self}")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The version in an answer of [`LATEST_RELEASE_API`], if the answer can be read.
pub fn latest_version(answer: &[u8]) -> Option<Version> {
    let answer: serde_json::Value = serde_json::from_slice(answer).ok()?;
    Version::parse(answer.get("tag_name")?.as_str()?)
}

/// What a release's signature covers: the file's name and contents, so an older signed installer can't pass for a
/// newer version (REQ-031-09).
pub fn signed_message(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(name.len() + 1 + contents.len());
    message.extend_from_slice(name.as_bytes());
    message.push(b'\n');
    message.extend_from_slice(contents);
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn req_031_04_compares_each_part_as_a_number() {
        assert!(version("0.10.0") > version("0.9.9"));
        assert!(version("1.0.0") > version("0.99.99"));
        assert!(version("v0.4.1") > version("0.4.0"));
        assert_eq!(version("v0.4.0"), version("0.4.0"));
    }

    /// REQ-031-10: whatever the answer says can't change where a download comes from.
    #[test]
    fn req_031_10_reads_only_plain_versions() {
        for text in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1.2.3-beta",
            "+1.2.3",
            "1.2.3/../../evil",
            "vv1.2.3",
            "99999999999.0.0",
        ] {
            assert_eq!(Version::parse(text), None, "{text}");
        }
    }

    #[test]
    fn req_031_10_downloads_only_from_this_repositorys_releases() {
        let version = version("1.2.3");
        assert_eq!(
            version.installer_url(),
            "https://github.com/Flayms/Good-Gallery/releases/download/v1.2.3/good-gallery-1.2.3-windows-x64-setup.exe"
        );
        assert_eq!(version.signature_url(), format!("{}.sig", version.installer_url()));
        assert_eq!(
            version.release_url(),
            "https://github.com/Flayms/Good-Gallery/releases/tag/v1.2.3"
        );
    }

    #[test]
    fn req_031_03_an_unreadable_answer_has_no_version() {
        assert_eq!(
            latest_version(br#"{"tag_name":"v1.2.3","draft":false}"#),
            Some(version("1.2.3"))
        );
        for answer in [
            &b""[..],
            b"<html>",
            br#"{"message":"API rate limit exceeded"}"#,
            br#"{"tag_name":12}"#,
            br#"{"tag_name":"nightly"}"#,
        ] {
            assert_eq!(latest_version(answer), None);
        }
    }

    #[test]
    fn req_031_09_the_signature_covers_the_name_and_the_contents() {
        assert_eq!(signed_message("a.exe", b"bytes"), b"a.exe\nbytes");
    }
}
