//! Opt-in elevated, current-user logon task. Never changed during startup.

#[cfg(windows)]
pub fn configure(on: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let script = if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.to_string_lossy().replace('\'', "''");
        format!(r#"
$ErrorActionPreference = 'Stop'
$user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$action = New-ScheduledTaskAction -Execute '{exe}' -Argument '--start-hidden'
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName 'Aion Tallymark' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
"#)
    } else {
        r#"
$ErrorActionPreference = 'Stop'
$task = Get-ScheduledTask -TaskPath '\' | Where-Object TaskName -EQ 'Aion Tallymark'
if ($task) { $task | Unregister-ScheduledTask -Confirm:$false }
"#.to_owned()
    };
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| format!("Could not update Start with Windows: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    // Not elevated: rerun the same script once through a UAC prompt.
    run_elevated(&script)
}

#[cfg(windows)]
fn run_elevated(script: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let path = std::env::temp_dir().join(format!("aion2dps-startup-{}.ps1", std::process::id()));
    std::fs::write(&path, script).map_err(|e| format!("Could not update Start with Windows: {e}"))?;
    let file = path.to_string_lossy().replace('\'', "''");
    let launcher = format!(
        "$p = Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{file}\"'; exit $p.ExitCode"
    );
    let result = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &launcher])
        .creation_flags(0x0800_0000)
        .output();
    let _ = std::fs::remove_file(&path);
    let output = result.map_err(|e| format!("Could not update Start with Windows: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err("Could not update Start with Windows. Approve the administrator prompt, or run the app as administrator.".into())
    }
}

#[cfg(not(windows))]
pub fn configure(_on: bool) -> Result<(), String> {
    Err("Start with Windows is only supported on Windows.".into())
}