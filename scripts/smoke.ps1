# Drives Tendril on a real Windows desktop (a GitHub runner) the way a person would, and writes
# down what happened: screenshots, what had focus, how long typed text took to show, memory.
# .github/workflows/smoke.yml runs it and keeps the results on the smoke-results branch.
#
#   pwsh scripts/smoke.ps1 -Exe target\release\tendril.exe -Out smoke

param([string]$Exe = "target\release\tendril.exe", [string]$Out = "smoke")
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class W {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  public static string Title(IntPtr h) { var s = new StringBuilder(256); GetWindowText(h, s, 256); return s.ToString(); }
}
"@

$report = Join-Path $Out "report.txt"
"Tendril smoke test, $(Get-Date -Format o)" | Set-Content $report
function Note($text) { $text | Tee-Object -Append -FilePath $report }
function Shot($name) {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
  $bmp.Save((Join-Path $Out "$name.png"))
  $g.Dispose(); $bmp.Dispose()
}
function Window { [W]::FindWindow($null, "Tendril") }
function Visible { $h = Window; ($h -ne [IntPtr]::Zero) -and [W]::IsWindowVisible($h) }
function Front { [W]::Title([W]::GetForegroundWindow()) }
function Keys($k) { [System.Windows.Forms.SendKeys]::SendWait($k) }
function WaitFor([scriptblock]$cond, [int]$ms = 5000) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($sw.ElapsedMilliseconds -lt $ms) { if (& $cond) { return $sw.ElapsedMilliseconds }; Start-Sleep -Milliseconds 10 }
  return -1
}
# The search field's text, read through UI Automation (Slint's accessibility).
function FieldText {
  try {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle((Window))
    $edit = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
      (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)))
    if ($edit) { return $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value }
  } catch {}
  return $null
}
function Memory {
  $p = Get-Process tendril -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($p) { "{0:N1} MB private, {1:N1} MB working set" -f ($p.PrivateMemorySize64 / 1MB), ($p.WorkingSet64 / 1MB) } else { "not running" }
}

# 1. First start: the walkthrough should be in front, with focus.
$sw = [Diagnostics.Stopwatch]::StartNew()
Start-Process $Exe
$shown = WaitFor { Visible } 10000
Note "start: window visible after $shown ms (-1 = never)"
Start-Sleep -Milliseconds 800
Note "start: in front: '$(Front)'"
Shot "01-first-start"

# 2. The shortcut step: record Ctrl+Shift+K.
Keys "{ENTER}"; Start-Sleep -Milliseconds 500
Shot "02-shortcut-step"
Keys "^+k"; Start-Sleep -Milliseconds 600
Shot "03-recorded"
$config = Join-Path $env:APPDATA "tendril\config.toml"
$hotkey = (Select-String -Path $config -Pattern '^hotkey' -ErrorAction SilentlyContinue).Line
Note "walkthrough: recorded shortcut, config says: $hotkey"

# 3. Finish the walkthrough.
foreach ($i in 1..4) { Keys "{ENTER}"; Start-Sleep -Milliseconds 300 }
Start-Sleep -Milliseconds 500
Note "walkthrough: done, visible: $(Visible), in front: '$(Front)'"
Shot "04-home"

# 4. Typing quickly: how long until the field shows all of it.
$text = "visual studio code"
$sw.Restart()
foreach ($c in $text.ToCharArray()) { Keys ([string]$c); Start-Sleep -Milliseconds 15 }
$typed = $sw.ElapsedMilliseconds
$caught = WaitFor { (FieldText) -eq $text } 5000
Note "typing: sent '$text' in $typed ms; field caught up $caught ms after the last key (-1 = never; field reads '$(FieldText)')"
Shot "05-typed"
Start-Sleep -Milliseconds 800
Shot "06-typed-later"

# 5. Clicking another window should close Tendril.
Start-Process notepad; $np = WaitFor { (Front) -match "Notepad" } 5000
Start-Sleep -Milliseconds 800
Note "click away: Notepad in front after $np ms; Tendril visible: $(Visible)"
Shot "07-after-click-away"

# 6. The recorded shortcut opens it again, in front.
Keys "^+k"
$open = WaitFor { Visible } 3000
Start-Sleep -Milliseconds 400
Note "shortcut: visible after $open ms; in front: '$(Front)'; field: '$(FieldText)'"
Shot "08-shortcut-open"

# 7. Escape twice closes it; starting tendril.exe again brings it back.
Keys "{ESC}"; Start-Sleep -Milliseconds 200; Keys "{ESC}"; Start-Sleep -Milliseconds 400
Note "escape: visible: $(Visible)"
Start-Process $Exe
$again = WaitFor { Visible } 5000
Start-Sleep -Milliseconds 400
Note "start again: visible after $again ms; tendril processes: $((Get-Process tendril -ErrorAction SilentlyContinue).Count); in front: '$(Front)'"
Shot "09-started-again"

# 8. Clipboard history sees a copy.
Set-Clipboard -Value "smoke test copy"
Start-Sleep -Milliseconds 500
Keys "{ESC}"; Start-Sleep -Milliseconds 300
Keys "^+k"; Start-Sleep -Milliseconds 500
Keys "clipboard"; Start-Sleep -Milliseconds 300; Keys "{ENTER}"; Start-Sleep -Milliseconds 600
Shot "10-clipboard"

# 9. Memory while open, then closed for a while.
Note "memory open: $(Memory)"
Keys "{ESC}"; Start-Sleep -Milliseconds 200; Keys "{ESC}"; Start-Sleep -Seconds 5
Note "memory closed: $(Memory)"
$log = Join-Path $env:LOCALAPPDATA "tendril\tendril.log"
if (Test-Path $log) { Copy-Item $log $Out }
Get-Process tendril -ErrorAction SilentlyContinue | Stop-Process
Note "done"
