//! Native feasibility evidence for #3970; not used by the config writer.

use super::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use windows_sys::Win32::{
    Security::{
        DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{MoveFileExW, ReplaceFileW},
};

use super::config_file_tests::powershell;

struct Probe(PathBuf);
impl Probe {
    fn new(case: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "herdr-native-replace-{case}-{}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot(path: &Path) -> Vec<u16> {
    let information = OWNER_SECURITY_INFORMATION
        | GROUP_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | LABEL_SECURITY_INFORMATION;
    let mut descriptor = config_security_descriptor(path, information).unwrap();
    config_security_sddl(&mut descriptor, information).unwrap()
}

fn replace(source: &Path, temporary: &Path, backup: &Path) -> std::io::Result<()> {
    let source = extended_length_path(source)?;
    let temporary = extended_length_path(temporary)?;
    let backup = extended_length_path(backup)?;
    // Neither ACL-ignore flag is allowed. WRITE_THROUGH is unsupported here.
    if unsafe {
        ReplaceFileW(
            source.as_ptr(),
            temporary.as_ptr(),
            backup.as_ptr(),
            0,
            null_mut(),
            null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn restore_without_overwrite(backup: &Path, target: &Path) -> std::io::Result<()> {
    let backup = extended_length_path(backup)?;
    let target = extended_length_path(target)?;
    // Deliberately no REPLACE_EXISTING: recovery must not clobber an intervening file.
    if unsafe { MoveFileExW(backup.as_ptr(), target.as_ptr(), 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn seed_owner_and_group(source: &Path, temporary: &Path) {
    use windows_sys::Win32::Security::Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{GetSecurityDescriptorGroup, GetSecurityDescriptorOwner};
    let information = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION;
    let mut descriptor = config_security_descriptor(source, information).unwrap();
    let descriptor = descriptor.as_mut_ptr().cast();
    let mut owner = null_mut();
    let mut group = null_mut();
    let mut defaulted = 0;
    assert_ne!(
        unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) },
        0
    );
    assert_ne!(
        unsafe { GetSecurityDescriptorGroup(descriptor, &mut group, &mut defaulted) },
        0
    );
    let temporary = extended_length_path(temporary).unwrap();
    let error = unsafe {
        SetNamedSecurityInfoW(
            temporary.as_ptr(),
            SE_FILE_OBJECT,
            information,
            owner,
            group,
            null_mut(),
            null_mut(),
        )
    };
    assert_eq!(
        error,
        0,
        "non-DACL metadata setup: {}",
        std::io::Error::from_raw_os_error(error as i32)
    );
}

fn probe_success(case: &str, seed_metadata: bool) {
    let dir = Probe::new(case);
    let source = dir.0.join("source");
    std::fs::write(&source, b"original preferences").unwrap();
    std::fs::write(dir.0.join("source:private"), b"original stream").unwrap();
    match case {
        "protected" | "unprotected" => {
            let script = format!(
                r#"
$ErrorActionPreference = 'Stop'
$acl = [System.IO.File]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE)
$acl.SetAccessRuleProtection(${}, $false)
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new($sid, [System.Security.AccessControl.FileSystemRights]::FullControl, [System.Security.AccessControl.AccessControlType]::Allow)
$acl.AddAccessRule($rule)
[System.IO.File]::SetAccessControl($env:HERDR_TEST_CONFIG_SOURCE, $acl)
"#,
                case == "protected"
            );
            powershell(&script, &source);
        }
        "metadata-bare" | "metadata-seeded" => {
            powershell(
                r#"
$ErrorActionPreference = 'Stop'
$acl = [System.IO.File]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE)
$acl.SetOwner([System.Security.Principal.WindowsIdentity]::GetCurrent().User)
$acl.SetGroup([System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'))
[System.IO.File]::SetAccessControl($env:HERDR_TEST_CONFIG_SOURCE, $acl)
$null = & icacls.exe $env:HERDR_TEST_CONFIG_SOURCE /setintegritylevel L
if ($LASTEXITCODE -ne 0) { throw 'could not install low integrity label' }
"#,
                &source,
            );
        }
        _ => {}
    }
    let before = snapshot(&source);
    if case == "legacy" {
        assert!(
            String::from_utf16_lossy(&before).contains("D:("),
            "legacy fixture: {}",
            String::from_utf16_lossy(&before)
        );
    }
    let source = if case == "moved" {
        powershell(
            r#"
$ErrorActionPreference = 'Stop'
$acl = [System.IO.File]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE)
$acl.SetAccessRuleProtection($false, $false)
[System.IO.File]::SetAccessControl($env:HERDR_TEST_CONFIG_SOURCE, $acl)
"#,
            &source,
        );
        let retained = snapshot(&source);
        let parent = dir.0.join("broader-parent");
        std::fs::create_dir(&parent).unwrap();
        powershell(
            r#"
$ErrorActionPreference = 'Stop'
$acl = [System.IO.Directory]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE)
$sid = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-546')
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new($sid, [System.Security.AccessControl.FileSystemRights]::Read, [System.Security.AccessControl.InheritanceFlags]::ObjectInherit, [System.Security.AccessControl.PropagationFlags]::None, [System.Security.AccessControl.AccessControlType]::Allow)
$acl.AddAccessRule($rule)
[System.IO.Directory]::SetAccessControl($env:HERDR_TEST_CONFIG_SOURCE, $acl)
"#,
            &parent,
        );
        let moved = parent.join("source");
        std::fs::rename(&source, &moved).unwrap();
        assert_eq!(snapshot(&moved), retained);
        assert!(!String::from_utf16_lossy(&retained).contains(";;;BG)"));
        let inherited = parent.join("inherited");
        std::fs::write(&inherited, b"").unwrap();
        assert!(String::from_utf16_lossy(&snapshot(&inherited)).contains(";;;BG)"));
        moved
    } else {
        source
    };
    let expected = snapshot(&source);
    let expected_text = String::from_utf16_lossy(&expected);
    match case {
        "protected" => assert!(expected_text.contains("D:PAI"), "{expected_text}"),
        "unprotected" | "moved" => {
            assert!(expected_text.contains("D:AI"), "{expected_text}");
            assert!(!expected_text.contains("D:P"), "{expected_text}");
        }
        _ => {}
    }
    let parent = source.parent().unwrap();
    let temporary = parent.join("replacement");
    let backup = parent.join("backup");
    let mut stage = create_config_temporary(&temporary, true).unwrap();
    if case.starts_with("metadata-") {
        let initial = snapshot(&temporary);
        let initial_text = String::from_utf16_lossy(&initial);
        let expected_identity = expected_text.split("D:").next().unwrap();
        let initial_identity = initial_text.split("D:").next().unwrap();
        assert_ne!(
            expected_identity.split("G:").next(),
            initial_identity.split("G:").next(),
            "source owner must differ from staging defaults"
        );
        assert_ne!(
            expected_identity.split("G:").nth(1),
            initial_identity.split("G:").nth(1),
            "source group must differ from staging defaults"
        );
        assert!(String::from_utf16_lossy(&expected).contains(";;;LW)"));
        if seed_metadata {
            // The bare probe retained the low label but not owner/group.
            // Test only that missing supplement; do not reapply DACLs or labels.
            seed_owner_and_group(&source, &temporary);
        }
    }
    stage.write_all(b"complete new preferences").unwrap();
    stage.sync_all().unwrap();
    drop(stage);
    // Permission setup must not make the source itself unwritable.
    drop(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&source)
            .unwrap(),
    );
    replace(&source, &temporary, &backup).unwrap();
    let actual = snapshot(&source);
    let backup_security = snapshot(&backup);
    eprintln!(
        "{case}: expected {}; actual {}; backup {}",
        String::from_utf16_lossy(&expected),
        String::from_utf16_lossy(&actual),
        String::from_utf16_lossy(&backup_security)
    );
    assert_eq!(std::fs::read(&source).unwrap(), b"complete new preferences");
    assert_eq!(std::fs::read(&backup).unwrap(), b"original preferences");
    assert_eq!(
        std::fs::read(parent.join("source:private")).unwrap(),
        b"original stream"
    );
    assert_eq!(
        std::fs::read(parent.join("backup:private")).unwrap(),
        b"original stream"
    );
    assert!(!temporary.exists());
    assert_eq!(backup_security, expected, "backup security");
    assert_eq!(actual, expected, "replacement security");
}

#[test]
fn native_replace_probe_legacy() {
    probe_success("legacy", false);
}
#[test]
fn native_replace_probe_protected() {
    probe_success("protected", false);
}
#[test]
fn native_replace_probe_unprotected() {
    probe_success("unprotected", false);
}
#[test]
fn native_replace_probe_moved() {
    probe_success("moved", false);
}
#[test]
fn native_replace_probe_metadata_bare() {
    probe_success("metadata-bare", false);
}
#[test]
fn native_replace_probe_metadata_seeded() {
    probe_success("metadata-seeded", true);
}

#[test]
fn native_replace_probe_failure_and_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    let dir = Probe::new("recovery");
    let source = dir.0.join("source");
    let temporary = dir.0.join("replacement");
    let backup = dir.0.join("backup");
    std::fs::write(&source, b"original").unwrap();
    let expected = snapshot(&source);
    let mut stage = create_config_temporary(&temporary, true).unwrap();
    stage.write_all(b"new").unwrap();
    stage.sync_all().unwrap();
    drop(stage);
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&source)
        .unwrap();
    let error = replace(&source, &temporary, &backup).unwrap_err();
    eprintln!("native sharing failure: {error}");
    assert_eq!(std::fs::read(&source).unwrap(), b"original");
    assert_eq!(snapshot(&source), expected);
    assert_eq!(std::fs::read(&temporary).unwrap(), b"new");
    assert!(!backup.exists());
    drop(held);
    replace(&source, &temporary, &backup).unwrap();
    // Construct the documented 1177 recovery layout; this is not fault injection.
    std::fs::remove_file(&source).unwrap();
    std::fs::write(&source, b"intervening update").unwrap();
    assert!(restore_without_overwrite(&backup, &source).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), b"intervening update");
    assert_eq!(std::fs::read(&backup).unwrap(), b"original");
    assert_eq!(snapshot(&backup), expected);
    std::fs::remove_file(&source).unwrap();
    restore_without_overwrite(&backup, &source).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), b"original");
    assert_eq!(snapshot(&source), expected);
    assert!(!backup.exists());
}
