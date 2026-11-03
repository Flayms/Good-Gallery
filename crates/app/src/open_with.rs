//! _Open with_ entries for photos and videos on Windows (REQ-027-05, REQ-027-06): the keys the installer writes
//! (`packaging/windows/installer.nsi`), so the settings can turn them on and off.

/// The app's _Open with_ entries, or a fake in tests.
pub trait OpenWithEntries: Send + Sync {
    /// Whether Explorer offers the app under _Open with_ now.
    fn is_on(&self) -> bool;
    /// Adds or removes the entries, and keeps the choice for the installer's next update (REQ-014-15).
    fn set(&self, on: bool) -> Result<(), String>;
}

#[cfg(windows)]
pub use registry::Registry;

#[cfg(windows)]
#[allow(unsafe_code)]
mod registry {
    use super::OpenWithEntries;
    use std::path::{Path, PathBuf};

    /// The installed app's entries, under `base` in the current user's registry (`Software`, a test key in tests).
    pub struct Registry {
        base: String,
        exe: PathBuf,
    }

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    };

    /// As in the installer: the file type _Open with_ lists, and the app's own key.
    const PROGID: &str = "GoodGallery.Media";
    const APPLICATION: &str = r"Applications\good-gallery.exe";
    const UNINSTALL_KEY: &str = r"Microsoft\Windows\CurrentVersion\Uninstall\GoodGallery";

    impl Registry {
        /// The installed app's entries; `None` unless this executable is the one the installer put in place (the
        /// portable app registers nothing, REQ-027-05).
        pub fn installed() -> Option<Self> {
            let exe = std::env::current_exe().ok()?;
            let location = string(&format!(r"Software\{UNINSTALL_KEY}"), "InstallLocation")?;
            let installed = same_folder(exe.parent()?, Path::new(&location));
            installed.then(|| Self {
                base: "Software".into(),
                exe,
            })
        }

        #[cfg(test)]
        pub(super) fn under(base: String, exe: PathBuf) -> Self {
            Self { base, exe }
        }

        #[cfg(test)]
        pub(super) fn base(&self) -> &str {
            &self.base
        }

        fn classes(&self, key: &str) -> String {
            format!(r"{}\Classes\{key}", self.base)
        }

        fn add(&self) -> Result<(), WIN32_ERROR> {
            let exe = self.exe.display();
            let command = format!(r#""{exe}" "%1""#);
            set_string(&self.classes(PROGID), "", "Photo or video")?;
            set_string(
                &self.classes(&format!(r"{PROGID}\DefaultIcon")),
                "",
                &format!("{exe},0"),
            )?;
            set_string(&self.classes(&format!(r"{PROGID}\shell\open\command")), "", &command)?;
            set_string(&self.classes(APPLICATION), "FriendlyAppName", "Good Gallery")?;
            set_string(
                &self.classes(&format!(r"{APPLICATION}\shell\open\command")),
                "",
                &command,
            )?;
            for extension in extensions() {
                set_string(&self.classes(&format!(r"{extension}\OpenWithProgids")), PROGID, "")?;
                set_string(&self.classes(&format!(r"{APPLICATION}\SupportedTypes")), extension, "")?;
            }
            Ok(())
        }

        fn remove(&self) -> Result<(), WIN32_ERROR> {
            for extension in extensions() {
                delete_value(&self.classes(&format!(r"{extension}\OpenWithProgids")), PROGID)?;
            }
            delete_tree(&self.classes(PROGID))?;
            delete_tree(&self.classes(APPLICATION))
        }
    }

    impl OpenWithEntries for Registry {
        fn is_on(&self) -> bool {
            exists(&self.classes(&format!(r"{PROGID}\shell\open\command")))
        }

        fn set(&self, on: bool) -> Result<(), String> {
            let changed = if on { self.add() } else { self.remove() };
            changed
                .and_then(|()| set_dword(&format!(r"{}\{UNINSTALL_KEY}", self.base), "OpenWith", u32::from(on)))
                .map_err(|status| std::io::Error::from_raw_os_error(status as i32).to_string())?;
            // SHCNE_ASSOCCHANGED: Explorer reads the associations again, as after the installer.
            // SAFETY: no item lists are passed with this event.
            unsafe {
                windows_sys::Win32::UI::Shell::SHChangeNotify(
                    windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32,
                    windows_sys::Win32::UI::Shell::SHCNF_IDLIST,
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            Ok(())
        }
    }

    fn extensions() -> impl Iterator<Item = &'static str> {
        domain::media::IMAGE_EXTENSIONS
            .iter()
            .chain(&domain::media::VIDEO_EXTENSIONS)
            .copied()
    }

    /// Whether two folders are the same, as Windows compares paths: in any letter case, with or without a trailing
    /// separator.
    fn same_folder(a: &Path, b: &Path) -> bool {
        let normal = |path: &Path| {
            let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            path.to_string_lossy().trim_end_matches('\\').to_lowercase()
        };
        normal(a) == normal(b)
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(status: WIN32_ERROR) -> Result<(), WIN32_ERROR> {
        if status == ERROR_SUCCESS { Ok(()) } else { Err(status) }
    }

    /// Opens `key`, creating it if it isn't there, and passes it to `write`.
    fn with_created(key: &str, write: impl FnOnce(HKEY) -> WIN32_ERROR) -> Result<(), WIN32_ERROR> {
        let key = wide(key);
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: the key name is NUL-terminated and lives for the call; `handle` receives the opened key.
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &raw mut handle,
                std::ptr::null_mut(),
            )
        })?;
        let status = write(handle);
        // SAFETY: `handle` was opened above and is closed once.
        unsafe { RegCloseKey(handle) };
        check(status)
    }

    /// Sets a string value; `""` names the key's default value.
    fn set_string(key: &str, name: &str, value: &str) -> Result<(), WIN32_ERROR> {
        let (name, value) = (wide(name), wide(value));
        with_created(key, |handle| {
            // SAFETY: `handle` is open for setting values; the name is NUL-terminated, and the data is the
            // NUL-terminated UTF-16 string with its size in bytes.
            unsafe {
                RegSetValueExW(
                    handle,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            }
        })
    }

    fn set_dword(key: &str, name: &str, value: u32) -> Result<(), WIN32_ERROR> {
        let name = wide(name);
        with_created(key, |handle| {
            // SAFETY: `handle` is open for setting values; the name is NUL-terminated and the data is 4 bytes.
            unsafe {
                RegSetValueExW(
                    handle,
                    name.as_ptr(),
                    0,
                    REG_DWORD,
                    (&raw const value).cast(),
                    size_of::<u32>() as u32,
                )
            }
        })
    }

    /// Deletes a value; one that isn't there is fine.
    fn delete_value(key: &str, name: &str) -> Result<(), WIN32_ERROR> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both strings are NUL-terminated and live for the call.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(status)
        }
    }

    /// Deletes a key with everything in it; one that isn't there is fine.
    fn delete_tree(key: &str) -> Result<(), WIN32_ERROR> {
        let key = wide(key);
        // SAFETY: the string is NUL-terminated and lives for the call.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(status)
        }
    }

    fn exists(key: &str) -> bool {
        let key = wide(key);
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: the key name is NUL-terminated and lives for the call; `handle` receives the opened key.
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &raw mut handle) };
        if status == ERROR_SUCCESS {
            // SAFETY: `handle` was opened above and is closed once.
            unsafe { RegCloseKey(handle) };
        }
        status == ERROR_SUCCESS
    }

    /// A string value of `key`, if there is one.
    fn string(key: &str, name: &str) -> Option<String> {
        let (key, name) = (wide(key), wide(name));
        let mut buffer = vec![0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: both strings are NUL-terminated and live for the call; `buffer` holds `size` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        (status == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegDeleteKeyW, RegDeleteTreeW, RegGetValueW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A string value (`""` for the key's default), if there is one.
    #[allow(unsafe_code)]
    fn string(key: &str, name: &str) -> Option<String> {
        let (key, name) = (wide(key), wide(name));
        let mut buffer = vec![0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: both strings are NUL-terminated and live for the call; `buffer` holds `size` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        (status == 0).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
    }

    #[allow(unsafe_code)]
    fn dword(key: &str, name: &str) -> Option<u32> {
        let (key, name) = (wide(key), wide(name));
        let mut value = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: both strings are NUL-terminated and live for the call; `value` has room for the DWORD asked for.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut value).cast(),
                &raw mut size,
            )
        };
        (status == 0).then_some(value)
    }

    /// Removes the test's registry key when the test ends.
    struct TestKey(String);

    impl Drop for TestKey {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            let key = wide(&self.0);
            // The parent only once no other test uses it: deleting a key with subkeys fails.
            let parent = wide(r"Software\GoodGalleryTests");
            // SAFETY: the strings are NUL-terminated and live for the calls.
            unsafe {
                RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
                RegDeleteKeyW(HKEY_CURRENT_USER, parent.as_ptr());
            }
        }
    }

    /// REQ-027-06: the switch adds the installer's entries for every indexed format, removes them again, and keeps the
    /// choice where the installer looks for it.
    #[test]
    fn req_027_06_adds_and_removes_the_installers_entries() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let test_key = TestKey(format!(r"Software\GoodGalleryTests\{}-{nanos}", std::process::id()));
        let exe = PathBuf::from(r"C:\Apps\Good Gallery\good-gallery.exe");
        let registry = Registry::under(test_key.0.clone(), exe);
        let classes = format!(r"{}\Classes", registry.base());
        let app_key = format!(r"{classes}\Applications\good-gallery.exe");
        let uninstall = format!(
            r"{}\Microsoft\Windows\CurrentVersion\Uninstall\GoodGallery",
            registry.base()
        );
        let extensions = domain::media::IMAGE_EXTENSIONS
            .iter()
            .chain(&domain::media::VIDEO_EXTENSIONS);
        assert!(!registry.is_on());

        registry.set(true).unwrap();
        assert!(registry.is_on());
        assert_eq!(
            string(&format!(r"{classes}\GoodGallery.Media\shell\open\command"), "").as_deref(),
            Some(r#""C:\Apps\Good Gallery\good-gallery.exe" "%1""#)
        );
        assert_eq!(string(&app_key, "FriendlyAppName").as_deref(), Some("Good Gallery"));
        for extension in extensions.clone() {
            let progids = format!(r"{classes}\{extension}\OpenWithProgids");
            assert_eq!(
                string(&progids, "GoodGallery.Media").as_deref(),
                Some(""),
                "{extension}"
            );
            assert_eq!(
                string(&format!(r"{app_key}\SupportedTypes"), extension).as_deref(),
                Some("")
            );
        }
        assert_eq!(dword(&uninstall, "OpenWith"), Some(1));

        registry.set(false).unwrap();
        assert!(!registry.is_on());
        assert_eq!(string(&app_key, "FriendlyAppName"), None);
        for extension in extensions {
            let progids = format!(r"{classes}\{extension}\OpenWithProgids");
            assert_eq!(string(&progids, "GoodGallery.Media"), None, "{extension}");
        }
        assert_eq!(dword(&uninstall, "OpenWith"), Some(0));
    }
}
