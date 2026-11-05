# Summarize %LOCALAPPDATA%\aion2dps\death-debug.jsonl (written when AION2DPS_DEBUG_DEATHS=1).
# Usage: powershell -ExecutionPolicy Bypass -File scripts\summarize-death-debug.ps1 [path]
param([string]$Path = (Join-Path $env:LOCALAPPDATA 'aion2dps\death-debug.jsonl'))
if (-not (Test-Path $Path)) { Write-Host "No log at $Path"; exit 1 }
$lines = @(Get-Content $Path | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
$despawns = @($lines | Where-Object { $_.kind -like 'despawn*' })
$hp = @($lines | Where-Object { $_.kind -like 'hp_*' })
Write-Host "Records: $($lines.Count)  (despawn: $($despawns.Count), hp: $($hp.Count))"
Write-Host "`nDespawns per kind/flag:"
$despawns | Group-Object kind, flag | Sort-Object Name | ForEach-Object { "  {0,-16} {1}" -f $_.Name, $_.Count } | Write-Host
Write-Host "`nHP records at 0:"
$hp | Where-Object { $_.hp -eq 0 } | ForEach-Object {
  "  {0} id={1} {2} type={3} name={4} boss={5}" -f $_.wall, $_.id, $_.kind, $_.entity.monster_type, $_.entity.monster_name, $_.entity.boss } | Write-Host
Write-Host "`nFlag 3 despawns:"
$despawns | Where-Object { $_.flag -eq 3 } | ForEach-Object {
  $e = $_.entity
  "  {0} id={1} class={2} type={3} name={4} boss={5} hits={6} lastHP={7} msSinceHP>0={8} msSinceHit={9}" -f `
    $_.wall, $_.id, $e.class, $e.monster_type, $e.monster_name, $e.boss, $e.hits, $e.last_hp, $_.ms_since_last_nonzero_hp, $_.ms_since_last_hit } | Write-Host
