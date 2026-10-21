//! Machine registration of the installed ASIO bundle (ADR 0025).
//! The plan and rollback are OS-independent; only the registry adapter uses Win32.

use std::fs;
use std::io;
use std::path::Path;

pub(crate) const ASIO_KEY: &str = r"SOFTWARE\ASIO\Spatial SoftReceiver";
const DLL: &str = "ssrasio.dll";
pub(crate) fn installed_path(program_files: &Path) -> std::path::PathBuf {
    program_files
        .join("Spatial SoftReceiver")
        .join("ASIO")
        .join(DLL)
}

fn win32_path(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    }
}

#[derive(Debug)]
pub(crate) struct Bundle {
    dll: String,
}

impl Bundle {
    /// Only the running setup executable supplies this root, never the INF
    /// argument or working directory. Canonical siblings must remain siblings
    /// (a symlink/reparse point cannot redirect either binary out of the bundle).
    pub(crate) fn from_setup_exe(setup_exe: &Path) -> io::Result<Self> {
        if !setup_exe.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "asio_bundle_relative_setup",
            ));
        }
        let setup_exe = fs::canonicalize(setup_exe)?;
        if !setup_exe.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "asio_bundle_setup_not_file",
            ));
        }
        let root = setup_exe.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "asio_bundle_missing_parent")
        })?;
        Ok(Self {
            dll: bundle_file(root, DLL)?,
        })
    }

    pub(crate) fn copy_to(&self, destination: &Path) -> io::Result<Self> {
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("asio_destination"))?;
        fs::create_dir_all(parent)?;
        fs::copy(&self.dll, destination)?;
        let dll = destination
            .to_str()
            .ok_or_else(|| io::Error::other("asio_destination_unicode"))?;
        Ok(Self {
            dll: win32_path(dll),
        })
    }

    fn plan(&self) -> Vec<KeyPlan> {
        let class = class_key();
        vec![
            KeyPlan {
                path: class.clone(),
                values: Vec::new(),
            },
            KeyPlan {
                path: format!(r"{class}\InprocServer32"),
                values: vec![
                    ("", self.dll.clone()),
                    ("ThreadingModel", "Apartment".into()),
                ],
            },
            KeyPlan {
                path: ASIO_KEY.into(),
                values: vec![
                    ("CLSID", asio_proto::CLSID.into()),
                    (
                        "Description",
                        ssr_i18n::names::product(ssr_types::app::Language::En).into(),
                    ),
                ],
            },
        ]
    }
}

fn bundle_file(root: &Path, name: &str) -> io::Result<String> {
    let candidate = root.join(name);
    let path = fs::canonicalize(&candidate).map_err(|error| {
        io::Error::new(error.kind(), format!("{}: {error}", candidate.display()))
    })?;
    if path.parent() != Some(root) || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("asio_bundle_not_sibling_file:{name}"),
        ));
    }
    path.into_os_string().into_string().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("asio_bundle_non_unicode:{name}"),
        )
    })
}

fn class_key() -> String {
    format!(r"SOFTWARE\Classes\CLSID\{}", asio_proto::CLSID)
}

#[derive(Debug)]
struct KeyPlan {
    path: String,
    values: Vec<(&'static str, String)>,
}

/// All operations use HKLM's 64-bit view. Deletion is non-recursive so an
/// unexpected child is reported, not erased. The fake implements the same rule.
pub(crate) trait Registry {
    type Error;
    fn exists(&mut self, key: &str) -> Result<bool, Self::Error>;
    fn read(&mut self, key: &str, name: &str) -> Result<Option<String>, Self::Error>;
    fn create(&mut self, key: &str) -> Result<(), Self::Error>;
    fn write(&mut self, key: &str, name: &str, value: &str) -> Result<(), Self::Error>;
    fn delete_value(&mut self, key: &str, name: &str) -> Result<(), Self::Error>;
    fn delete_key(&mut self, key: &str) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub(crate) struct RegistrationError<E> {
    pub(crate) cause: E,
    pub(crate) rollback: Vec<E>,
}

#[derive(Debug)]
struct Previous {
    existed: bool,
    values: Vec<(&'static str, Option<String>)>,
}

/// Read every old value before writing anything. Failed repair restores the
/// previous registration, rather than deleting an otherwise working install.
pub(crate) fn register<R: Registry>(
    registry: &mut R,
    bundle: &Bundle,
) -> Result<(), RegistrationError<R::Error>> {
    let plan = bundle.plan();
    let mut previous = Vec::new();
    for key in &plan {
        let snapshot = (|| -> Result<Previous, R::Error> {
            let existed = registry.exists(&key.path)?;
            let mut values = Vec::new();
            for (name, _) in &key.values {
                values.push((
                    *name,
                    if existed {
                        registry.read(&key.path, name)?
                    } else {
                        None
                    },
                ));
            }
            Ok(Previous { existed, values })
        })()
        .map_err(|cause| RegistrationError {
            cause,
            rollback: Vec::new(),
        })?;
        previous.push(snapshot);
    }
    for (index, key) in plan.iter().enumerate() {
        let outcome = (|| -> Result<(), R::Error> {
            registry.create(&key.path)?;
            for (name, value) in &key.values {
                registry.write(&key.path, name, value)?;
            }
            Ok(())
        })();
        if let Err(cause) = outcome {
            let mut rollback = Vec::new();
            for (key, old) in plan[..=index].iter().zip(&previous).rev() {
                if old.existed {
                    for (name, value) in &old.values {
                        let restored = match value {
                            Some(value) => registry.write(&key.path, name, value),
                            None => registry.delete_value(&key.path, name),
                        };
                        if let Err(error) = restored {
                            rollback.push(error);
                        }
                    }
                } else if let Err(error) = registry.delete_key(&key.path) {
                    rollback.push(error);
                }
            }
            return Err(RegistrationError { cause, rollback });
        }
    }
    // No runtime consumer remains. Remove the pre-release per-user launch hint.
    registry
        .delete_value(ASIO_KEY, "ApplicationPath")
        .map_err(|cause| RegistrationError {
            cause,
            rollback: Vec::new(),
        })?;
    Ok(())
}

/// Delete only our exact identity/path. Never orphan the ASIO list entry if
/// deleting it fails (for example because an unexpected child exists).
pub(crate) fn unregister<R: Registry>(
    registry: &mut R,
    installed: &Path,
) -> Result<bool, R::Error> {
    let class = class_key();
    let server = format!(r"{class}\InprocServer32");
    let clsid = registry.read(ASIO_KEY, "CLSID")?;
    let path = registry.read(&server, "")?;
    if clsid.is_none() && path.is_none() {
        return Ok(true);
    }
    if clsid.as_deref() != Some(asio_proto::CLSID)
        || !path.as_deref().is_some_and(|path| {
            path.eq_ignore_ascii_case(&win32_path(&installed.to_string_lossy()))
        })
    {
        return Ok(false);
    }
    registry.delete_key(ASIO_KEY)?;
    registry.delete_key(&server)?;
    registry.delete_key(&class)?;
    Ok(true)
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct MachineRegistry;

#[cfg(windows)]
impl Registry for MachineRegistry {
    type Error = crate::win::WinError;

    fn exists(&mut self, key: &str) -> Result<bool, Self::Error> {
        Ok(crate::win::RegKey::open_machine_64(key)?.is_some())
    }

    fn read(&mut self, key: &str, name: &str) -> Result<Option<String>, Self::Error> {
        match crate::win::RegKey::open_machine_64(key)? {
            Some(key) => key.string(name),
            None => Ok(None),
        }
    }

    fn create(&mut self, key: &str) -> Result<(), Self::Error> {
        crate::win::RegKey::create_machine_64(key).map(|_| ())
    }

    fn write(&mut self, key: &str, name: &str, value: &str) -> Result<(), Self::Error> {
        crate::win::RegKey::create_machine_64(key)?.set_string(name, value)
    }

    fn delete_value(&mut self, key: &str, name: &str) -> Result<(), Self::Error> {
        match crate::win::RegKey::open_machine_64(key)? {
            Some(key) => key.delete_value(name),
            None => Ok(()),
        }
    }

    fn delete_key(&mut self, key: &str) -> Result<(), Self::Error> {
        crate::win::RegKey::delete_machine_64(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct FakeRegistry {
        keys: BTreeMap<String, BTreeMap<String, String>>,
        writes: usize,
        fail_write: Option<usize>,
        fail_read: bool,
        fail_delete: Vec<String>,
    }

    impl Registry for FakeRegistry {
        type Error = String;
        fn exists(&mut self, key: &str) -> Result<bool, String> {
            Ok(self.keys.contains_key(key))
        }
        fn read(&mut self, key: &str, name: &str) -> Result<Option<String>, String> {
            if self.fail_read {
                return Err("read".into());
            }
            Ok(self
                .keys
                .get(key)
                .and_then(|values| values.get(name))
                .cloned())
        }
        fn create(&mut self, key: &str) -> Result<(), String> {
            self.keys.entry(key.into()).or_default();
            Ok(())
        }
        fn write(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
            self.writes += 1;
            if self.fail_write == Some(self.writes) {
                return Err("write".into());
            }
            self.keys
                .entry(key.into())
                .or_default()
                .insert(name.into(), value.into());
            Ok(())
        }
        fn delete_value(&mut self, key: &str, name: &str) -> Result<(), String> {
            if let Some(values) = self.keys.get_mut(key) {
                values.remove(name);
            }
            Ok(())
        }
        fn delete_key(&mut self, key: &str) -> Result<(), String> {
            if self.fail_delete.iter().any(|blocked| blocked == key)
                || self
                    .keys
                    .keys()
                    .any(|child| child.starts_with(&format!(r"{key}\")))
            {
                return Err(key.into());
            }
            self.keys.remove(key);
            Ok(())
        }
    }

    fn bundle() -> Bundle {
        Bundle {
            dll: r"C:\Program Files\Spatial SoftReceiver\ASIO\ssrasio.dll".into(),
        }
    }

    #[test]
    fn registration_plan_has_fixed_identity_and_absolute_bundle_paths() {
        let bundle = bundle();
        let mut registry = FakeRegistry::default();
        registry
            .write(ASIO_KEY, "ApplicationPath", "old-user.exe")
            .unwrap();
        register(&mut registry, &bundle).unwrap();
        let server = &registry.keys[&format!(r"{}\InprocServer32", class_key())];
        assert_eq!(server[""], bundle.dll);
        assert_eq!(server["ThreadingModel"], "Apartment");
        let asio = &registry.keys[ASIO_KEY];
        assert_eq!(asio["CLSID"], asio_proto::CLSID);
        assert_eq!(asio["Description"], "Spatial SoftReceiver");
        assert!(!asio.contains_key("ApplicationPath"));
        assert_eq!(registry.keys.len(), 3);
        let expected = registry.keys.clone();
        register(&mut registry, &bundle).unwrap();
        assert_eq!(registry.keys, expected);
    }

    #[test]
    fn partial_install_rolls_back_every_written_value() {
        for fail_write in 1..=4 {
            let mut registry = FakeRegistry {
                fail_write: Some(fail_write),
                ..Default::default()
            };
            let error = register(&mut registry, &bundle()).unwrap_err();
            assert!(error.rollback.is_empty());
            assert!(registry.keys.is_empty());
        }
    }

    #[test]
    fn failed_repair_restores_previous_values_and_absence() {
        for fail_write in 1..=4 {
            let mut registry = FakeRegistry::default();
            register(&mut registry, &bundle()).unwrap();
            registry
                .keys
                .get_mut(ASIO_KEY)
                .unwrap()
                .remove("ApplicationPath");
            registry
                .keys
                .get_mut(ASIO_KEY)
                .unwrap()
                .insert("Other".into(), "untouched".into());
            let expected = registry.keys.clone();
            registry.fail_write = Some(registry.writes + fail_write);
            let updated = Bundle {
                dll: "new.dll".into(),
            };
            assert!(
                register(&mut registry, &updated)
                    .unwrap_err()
                    .rollback
                    .is_empty()
            );
            assert_eq!(registry.keys, expected);
        }
    }

    #[test]
    fn snapshot_failure_never_writes_and_rollback_failures_are_reported() {
        let mut registry = FakeRegistry::default();
        register(&mut registry, &bundle()).unwrap();
        let expected = registry.keys.clone();
        registry.fail_read = true;
        let writes = registry.writes;
        assert_eq!(
            register(&mut registry, &bundle()).unwrap_err().cause,
            "read"
        );
        assert_eq!(registry.keys, expected);
        assert_eq!(registry.writes, writes);

        let mut registry = FakeRegistry {
            fail_write: Some(4),
            fail_delete: vec![ASIO_KEY.into()],
            ..Default::default()
        };
        let error = register(&mut registry, &bundle()).unwrap_err();
        assert_eq!(error.cause, "write");
        assert_eq!(error.rollback, [ASIO_KEY]);
    }

    #[test]
    fn uninstall_refuses_replaced_clsid_or_server_path() {
        for (key, name) in [
            (ASIO_KEY.to_owned(), "CLSID"),
            (format!(r"{}\InprocServer32", class_key()), ""),
        ] {
            let mut registry = FakeRegistry::default();
            register(&mut registry, &bundle()).unwrap();
            registry.write(&key, name, "other").unwrap();
            let before = registry.keys.clone();
            assert!(!unregister(&mut registry, Path::new(&bundle().dll)).unwrap());
            assert_eq!(registry.keys, before);
        }
    }
    #[test]
    fn installation_copy_replaces_old_bytes_and_normalizes_win32_paths() {
        let files = TestBundle::new();
        let source = Bundle::from_setup_exe(&files.setup()).unwrap();
        let destination = installed_path(&files.0);
        source.copy_to(&destination).unwrap();
        fs::write(files.0.join(DLL), b"replacement").unwrap();
        source.copy_to(&destination).unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"replacement");
        assert_eq!(
            win32_path(r"\\?\C:\Program Files\x.dll"),
            r"C:\Program Files\x.dll"
        );
        assert_eq!(
            win32_path(r"\\?\UNC\host\share\x.dll"),
            r"\\host\share\x.dll"
        );
    }
    #[test]
    fn uninstall_is_idempotent_and_keeps_unrelated_registrations() {
        let mut registry = FakeRegistry::default();
        registry.create(r"SOFTWARE\ASIO\Other").unwrap();
        registry.create(r"SOFTWARE\Classes\CLSID\{other}").unwrap();
        let expected = registry.keys.clone();
        register(&mut registry, &bundle()).unwrap();
        assert!(unregister(&mut registry, Path::new(&bundle().dll)).unwrap());
        assert!(unregister(&mut registry, Path::new(&bundle().dll)).unwrap());
        assert_eq!(registry.keys, expected);
    }

    #[test]
    fn uninstall_preserves_class_when_asio_key_cannot_be_deleted() {
        let mut registry = FakeRegistry::default();
        register(&mut registry, &bundle()).unwrap();
        registry.create(&format!(r"{ASIO_KEY}\Unexpected")).unwrap();
        registry.fail_delete.push(class_key());
        assert_eq!(
            unregister(&mut registry, Path::new(&bundle().dll)),
            Err(ASIO_KEY.to_owned())
        );
        assert!(
            registry
                .keys
                .contains_key(&format!(r"{}\InprocServer32", class_key()))
        );
        assert!(
            registry
                .keys
                .contains_key(&format!(r"{ASIO_KEY}\Unexpected"))
        );
    }

    struct TestBundle(PathBuf);
    impl TestBundle {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "ssr-asio-bundle-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            for name in ["ssr-setup.exe", DLL] {
                fs::write(root.join(name), []).unwrap();
            }
            Self(root)
        }
        fn setup(&self) -> PathBuf {
            self.0.join("ssr-setup.exe")
        }
    }
    impl Drop for TestBundle {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bundle_paths_are_canonical_siblings_even_when_directory_has_spaces() {
        let files = TestBundle::new();
        let subdir = files.0.join("installation current");
        fs::create_dir(&subdir).unwrap();
        for name in ["ssr-setup.exe", DLL] {
            fs::write(subdir.join(name), []).unwrap();
        }
        let bundle = Bundle::from_setup_exe(&subdir.join("ssr-setup.exe")).unwrap();
        assert_eq!(
            Path::new(&bundle.dll),
            fs::canonicalize(subdir.join(DLL)).unwrap()
        );
        let installed = bundle.copy_to(&installed_path(&files.0)).unwrap();
        assert!(Path::new(&installed.dll).is_file());
        assert!(!installed.dll.starts_with(r"\\?\"));
    }

    #[test]
    fn bundle_rejects_missing_binaries_directories_and_relative_setup() {
        assert!(Bundle::from_setup_exe(Path::new("ssr-setup.exe")).is_err());
        let files = TestBundle::new();
        fs::remove_file(files.0.join(DLL)).unwrap();
        assert_eq!(
            Bundle::from_setup_exe(&files.setup()).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::create_dir(files.0.join(DLL)).unwrap();
        assert!(Bundle::from_setup_exe(&files.setup()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn bundle_rejects_symlinks_outside_the_setup_directory() {
        let files = TestBundle::new();
        let outside = TestBundle::new();
        fs::remove_file(files.0.join(DLL)).unwrap();
        std::os::unix::fs::symlink(outside.0.join(DLL), files.0.join(DLL)).unwrap();
        assert!(Bundle::from_setup_exe(&files.setup()).is_err());
    }
}
