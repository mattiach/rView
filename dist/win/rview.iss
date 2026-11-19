; rView installer. Inno Setup 6: `iscc dist\win\rview.iss` after
; `cargo build --release -p rview`; the setup lands in `target\installer`.
;
; Per-user by default (no administrator rights, {autopf} = %LOCALAPPDATA%\Programs); the dialog
; offers "all users" too. File types: rView is added to "Open with" of every supported extension
; (OpenWithProgids) and registered in Settings > Apps > Default apps (Capabilities), without taking
; over the user's current defaults. `.rvm` projects have their own type.
;
; The [Registry] lines of the extensions are generated from core/src/formats.rs; the test
; `installer_lists_every_extension` (core/src/formats.rs) fails if they drift apart.
;
; Code signing (not done here: it needs your certificate): define a sign tool in the Inno Setup
; IDE or on the command line, e.g.
;   iscc "/Ssigntool=signtool.exe sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /a $f" dist\win\rview.iss
; and uncomment SignTool/SignedUninstaller below. Sign target\release\rview.exe the same way first.

#define AppName "rView"
#define AppVersion "1.5.0"
#define AppExe "rview.exe"
; the icon (made from assets\icon\rview.png by the build): setup, shortcuts and associated files use it
#define IconSrc AddBackslash(SourcePath) + "..\..\target\release\rview.ico"
#ifexist IconSrc
  #define AppIcon "{app}\rview.ico"
#else
  #define AppIcon "{app}\" + AppExe + ",0"
#endif

[Setup]
AppId={{6E1B3C52-0F0A-4C8E-9B67-2A3D5F1E7C90}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=mattiach
UninstallDisplayName={#AppName}
AppPublisherURL=https://github.com/mattiach/rview
AppSupportURL=https://github.com/mattiach/rview
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\..\target\installer
OutputBaseFilename=rview-{#AppVersion}-setup-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes
UninstallDisplayIcon={#AppIcon}
LicenseFile=..\..\LICENSE.md
#ifexist IconSrc
SetupIconFile={#IconSrc}
#endif
;SignTool=signtool
;SignedUninstaller=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\fonts\OFL.txt"; DestDir: "{app}\licenses"; DestName: "Lato-OFL.txt"; Flags: ignoreversion
Source: "..\..\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
#ifexist IconSrc
Source: "{#IconSrc}"; DestDir: "{app}"; Flags: ignoreversion
#endif

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{#AppIcon}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{#AppIcon}"; Tasks: desktopicon

[Registry]
; the file types
Root: HKA; Subkey: "Software\Classes\rView.Image"; ValueType: string; ValueName: ""; ValueData: "Image (rView)"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\rView.Image\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{#AppIcon}"
Root: HKA; Subkey: "Software\Classes\rView.Image\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\rView.Project"; ValueType: string; ValueName: ""; ValueData: "rView Markup project"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\rView.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{#AppIcon}"
Root: HKA; Subkey: "Software\Classes\rView.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
; "Open with" entry and Default apps (Windows 11)
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKA; Subkey: "Software\rView"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\rView\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"
Root: HKA; Subkey: "Software\rView\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Fast image viewer with Markup"
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#AppName}"; ValueData: "Software\rView\Capabilities"; Flags: uninsdeletevalue
; extensions (generated from core/src/formats.rs); keys that rView created and that stay empty are
; removed at uninstall, the ones other programs use are kept
Root: HKA; Subkey: "Software\Classes\.jpg"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.jpg\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpg"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".jpg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.jpeg"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.jpeg\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpeg"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".jpeg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.jpe"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.jpe\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpe"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".jpe"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.jfi"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.jfi\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jfi"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".jfi"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.jfif"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.jfif\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jfif"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".jfif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.png"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.png\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".png"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".png"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.gif"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.gif\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".gif"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".gif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.webp"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.webp\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".webp"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".webp"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.tif"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.tif\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".tif"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".tif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.tiff"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.tiff\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".tiff"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".tiff"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.bmp"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.bmp\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".bmp"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".bmp"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.ico"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.ico\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".ico"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".ico"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.cur"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.cur\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".cur"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".cur"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.icns"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.icns\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".icns"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".icns"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.tga"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.tga\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".tga"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".tga"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.pbm"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.pbm\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".pbm"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".pbm"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.pgm"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.pgm\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".pgm"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".pgm"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.ppm"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.ppm\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".ppm"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".ppm"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.pam"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.pam\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".pam"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".pam"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.qoi"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.qoi\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".qoi"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".qoi"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.hdr"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.hdr\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".hdr"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".hdr"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.exr"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.exr\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".exr"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".exr"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.dds"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.dds\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".dds"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".dds"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.jxl"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.jxl\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jxl"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".jxl"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.svg"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.svg\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".svg"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".svg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.svgz"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.svgz\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".svgz"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".svgz"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.heic"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.heic\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heic"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".heic"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.heics"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.heics\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heics"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".heics"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.heif"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.heif\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heif"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".heif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.heifs"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.heifs\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heifs"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".heifs"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.hif"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.hif\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".hif"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".hif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.avif"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.avif\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".avif"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".avif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.jxr"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.jxr\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jxr"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".jxr"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.wdp"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.wdp\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wdp"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".wdp"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.dng"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.dng\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".dng"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".dng"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.cr2"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.cr2\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".cr2"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".cr2"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.cr3"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.cr3\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".cr3"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".cr3"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.nef"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.nef\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".nef"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".nef"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.nrw"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.nrw\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".nrw"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".nrw"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.arw"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.arw\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".arw"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".arw"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.orf"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.orf\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".orf"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".orf"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.rw2"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.rw2\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".rw2"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".rw2"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.raf"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.raf\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".raf"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".raf"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.pef"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.pef\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".pef"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".pef"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.srw"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.srw\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".srw"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".srw"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.rvm"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.rvm\OpenWithProgids"; ValueType: string; ValueName: "rView.Project"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".rvm"; ValueData: "rView.Project"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".rvm"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.wbmp"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.wbmp\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wbmp"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".wbmp"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.xbm"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.xbm\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".xbm"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".xbm"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.xpm"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.xpm\OpenWithProgids"; ValueType: string; ValueName: "rView.Image"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\rView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".xpm"; ValueData: "rView.Image"
Root: HKA; Subkey: "Software\Classes\Applications\rview.exe\SupportedTypes"; ValueType: string; ValueName: ".xpm"; ValueData: ""

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
