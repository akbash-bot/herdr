use super::*;

pub(super) fn powershell(script: &str, source: &std::path::Path) -> String {
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("HERDR_TEST_CONFIG_SOURCE", source)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn config_replacement_preserves_windows_access_control() {
    let dir = std::env::temp_dir().join(format!("herdr-config-acl-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let source = dir.join("source");
    let target = dir.join("temporary");
    std::fs::write(&source, b"original").unwrap();
    powershell(
        r#"
$ErrorActionPreference = 'Stop'
# Use the .NET Framework API directly: a parent pwsh process can pass a
# PSModulePath containing incompatible PowerShell 7 versions of Get-Acl/Set-Acl.
$acl = [System.IO.File]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE)
$acl.SetAccessRuleProtection($true, $false)
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new($sid, [System.Security.AccessControl.FileSystemRights]::FullControl, [System.Security.AccessControl.AccessControlType]::Allow)
$acl.AddAccessRule($rule)
[System.IO.File]::SetAccessControl($env:HERDR_TEST_CONFIG_SOURCE, $acl)
"#,
        &source,
    );
    let snapshot = r#"$ErrorActionPreference = 'Stop'; [System.IO.File]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE).GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::All)"#;
    for protected in [true, false] {
        if !protected {
            powershell(
                r#"
$ErrorActionPreference = 'Stop'
$acl = [System.IO.File]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE)
$acl.SetAccessRuleProtection($false, $false)
[System.IO.File]::SetAccessControl($env:HERDR_TEST_CONFIG_SOURCE, $acl)
"#,
                &source,
            );
        }
        let before = powershell(snapshot, &source);
        drop(create_config_temporary(&target, true).unwrap());
        write_config_temporary(Some(&source), &target, b"new").unwrap();
        replace_file(&target, &source).unwrap();
        assert_eq!(
            powershell(snapshot, &source),
            before,
            "protected={protected}"
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"new");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn config_replacement_rejects_changed_inherited_access_before_copying_contents() {
    let dir = std::env::temp_dir().join(format!("herdr-config-moved-acl-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let parent = dir.join("different-parent");
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
    let source = dir.join("source");
    std::fs::write(&source, b"private preferences").unwrap();
    std::fs::write(source.with_file_name("source:private"), b"private stream").unwrap();
    let snapshot = r#"$ErrorActionPreference = 'Stop'; [System.IO.File]::GetAccessControl($env:HERDR_TEST_CONFIG_SOURCE).GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::All)"#;
    let before = powershell(snapshot, &source);
    assert!(!before.contains(";;;BG)"), "{before}");
    assert!(
        !before.contains("D:P"),
        "source must be unprotected: {before}"
    );
    let moved = parent.join("source");
    std::fs::rename(&source, &moved).unwrap();
    assert_eq!(powershell(snapshot, &moved), before, "move retains old ACL");
    let probe = parent.join("inherited");
    std::fs::write(&probe, b"").unwrap();
    assert!(powershell(snapshot, &probe).contains(";;;BG)"));
    let target = parent.join("temporary");
    drop(create_config_temporary(&target, true).unwrap());
    let error = write_config_temporary(Some(&moved), &target, b"new").unwrap_err();
    assert!(error
        .to_string()
        .contains("cannot preserve config access controls"));
    assert!(std::fs::read(&target).unwrap().is_empty());
    assert!(!target.with_file_name("temporary:private").exists());
    assert_eq!(std::fs::read(&moved).unwrap(), b"private preferences");
    assert_eq!(
        std::fs::read(moved.with_file_name("source:private")).unwrap(),
        b"private stream"
    );
    assert_eq!(powershell(snapshot, &moved), before);
    std::fs::remove_dir_all(dir).unwrap();
}
