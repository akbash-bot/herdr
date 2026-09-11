use super::*;

fn powershell(script: &str, source: &std::path::Path) -> String {
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
    let before = powershell(snapshot, &source);
    drop(create_config_temporary(&target, true).unwrap());
    write_config_temporary(Some(&source), &target, b"new").unwrap();
    replace_file(&target, &source).unwrap();
    assert_eq!(powershell(snapshot, &source), before);
    assert_eq!(std::fs::read(&source).unwrap(), b"new");
    std::fs::remove_dir_all(dir).unwrap();
}
