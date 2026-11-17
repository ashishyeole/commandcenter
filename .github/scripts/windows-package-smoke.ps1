param([switch]$LiveFeedAudit)
$ErrorActionPreference = 'Stop'
if ($LiveFeedAudit) {
  foreach ($base in @($env:APPDATA, $env:LOCALAPPDATA)) {
    if ($base -and (Test-Path (Join-Path $base 'com.personal.commandcenter'))) {
      throw 'Live bootstrap audit requires a fresh disposable runner profile'
    }
  }
}
$installer = Get-ChildItem target/release/bundle/nsis/*.exe | Select-Object -First 1
if (!$installer) { throw 'NSIS installer was not produced' }
$install = Join-Path $env:RUNNER_TEMP 'pcc-installed'
$report = [ordered]@{ platform='windows'; installer=$installer.Name; sha256=(Get-FileHash $installer.FullName -Algorithm SHA256).Hash; install_exit=$null; launch='not run'; uninstall_exit=$null; synthetic_runner=$true; full_feature_acceptance=$false }
$app = $null
try {
  $setup = Start-Process $installer.FullName -ArgumentList @('/S', "/D=$install") -Wait -PassThru
  $report.install_exit = $setup.ExitCode
  if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)" }
  $exe = Join-Path $install 'personal-command-center.exe'
  if (!(Test-Path $exe)) { throw 'Installed application executable was not found' }
  $app = Start-Process $exe -PassThru
  if ($app.WaitForExit(15000)) { throw "Application exited before launch smoke check: $($app.ExitCode)" }
  $app.Refresh()
  if ($app.MainWindowHandle -eq 0) { throw 'Application did not create a native window' }
  $report.launch='native window created and process stayed running for 15 seconds'
  $report.window_title=$app.MainWindowTitle
  if ($LiveFeedAudit) {
    python .github/scripts/windows-public-feed-audit.py
    if ($LASTEXITCODE -ne 0) { throw 'Installed public-feed bootstrap audit failed' }
    $report.public_feed_bootstrap='passed; see windows-public-feed-smoke.json for actual provider counts and failures'
  }
} finally {
  if ($app -and !$app.HasExited) { Stop-Process -Id $app.Id -Force; $app.WaitForExit(5000) | Out-Null }
  $uninstaller = Get-ChildItem $install -Filter '*uninstall*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($uninstaller) {
    $remove=Start-Process $uninstaller.FullName -ArgumentList '/S' -Wait -PassThru
    $report.uninstall_exit=$remove.ExitCode
  }
  $report | ConvertTo-Json -Depth 4 | Set-Content target/release/bundle/windows-launch-smoke.json
}
if ($report.uninstall_exit -ne 0) { throw 'Silent uninstall was not confirmed' }
