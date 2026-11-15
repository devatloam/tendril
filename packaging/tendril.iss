; Tendril's Windows installer (Inno Setup), built by .github/workflows/build.yml.
; Per user, no admin: Tendril goes in %LOCALAPPDATA%\Programs\Tendril, where its updater can replace
; it, with a Start menu entry, an entry in Settings > Apps to uninstall it, and, if you like, a
; start at sign-in written the same way Tendril's own "Open at login" writes it.

#define Version GetEnv("VERSION")
; Which tendril.exe to package: the release build's, unless TENDRIL_EXE says otherwise (smoke.yml).
#define Exe GetEnv("TENDRIL_EXE")
#if Exe == ""
  #define Exe "..\target\x86_64-pc-windows-msvc\release\tendril.exe"
#endif

[Setup]
AppId={{1B22A48F-20DA-4959-A9EB-F202B0B5A279}
AppName=Tendril
AppVersion={#Version}
AppVerName=Tendril {#Version}
AppPublisher=Loam
AppPublisherURL=https://github.com/devatloam/tendril
AppSupportURL=https://github.com/devatloam/tendril/issues
DefaultDirName={localappdata}\Programs\Tendril
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=tendril-{#Version}-windows-x86_64-setup
SetupIconFile=..\assets\tendril.ico
UninstallDisplayIcon={app}\tendril.exe
UninstallDisplayName=Tendril
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Tendril runs in the background: close it to update the file, start it again after.
CloseApplications=force
RestartApplications=no

[Tasks]
Name: "login"; Description: "Start Tendril when I sign in (hidden, ready for its shortcut)"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Tendril"; Filename: "{app}\tendril.exe"; Comment: "A keyboard launcher"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Tendril"; ValueData: """{app}\tendril.exe"" --hidden"; Tasks: login

[Run]
; Not in a silent install: whatever ran it (a script, a deployment tool) decides when Tendril starts.
Filename: "{app}\tendril.exe"; Description: "Open Tendril"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Stop it, and forget the start at sign-in however it was set (here or in Tendril's settings).
Filename: "{sys}\taskkill.exe"; Parameters: "/im tendril.exe /f"; Flags: runhidden; RunOnceId: "StopTendril"
Filename: "{sys}\reg.exe"; Parameters: "delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Tendril /f"; Flags: runhidden; RunOnceId: "ForgetLogin"
