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
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Could not update Start with Windows. Run the app as administrator. {}", details.trim()));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn configure(_on: bool) -> Result<(), String> {
    Err("Start with Windows is only supported on Windows.".into())
}
