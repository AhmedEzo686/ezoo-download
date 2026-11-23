; Ezoo Download — Windows installer (Inno Setup 6)
; Built automatically by BUILD.bat -> dist\EzooDownload-Setup.exe
;
; Installs for the current user (no admin needed) into
;   %LOCALAPPDATA%\Programs\Ezoo Download
; so yt-dlp can update itself. Adds a Start Menu folder, an optional desktop
; shortcut, "start with Windows", and a normal uninstaller.

#define MyAppName "Ezoo Download"
#define MyAppExe "EzooDownload.exe"
; BUILD.bat passes the version of Cargo.toml: /DMyAppVersion=1.0.1
; the files that go into the setup (gathered by BUILD.bat / the CI)
#ifndef Stage
  #define Stage "..\build-cache\stage"
#endif
#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif

[Setup]
AppId={{F5837666-2925-4FD9-8CE9-728BE9C5D7E4}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=Ahmed Ezzat
DefaultDirName={localappdata}\Programs\Ezoo Download
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
#ifdef Legacy
; the build for Windows 7 / 8 / 8.1 (tools/legacy): one 32-bit program for
; 32- and 64-bit Windows. Same program (same AppId, same folder): Windows 10
; or 11 put on later is updated to the build for them by the program
OutputBaseFilename=EzooDownload-Legacy-Setup
MinVersion=6.1sp1
#else
OutputBaseFilename=EzooDownload-Setup
; Windows 10 / 11 (Windows 7 / 8 / 8.1 have EzooDownload-Legacy-Setup.exe)
MinVersion=10.0
#endif
#ifndef NoIcon
SetupIconFile=..\crates\app\icon.ico
#endif
UninstallDisplayIcon={app}\{#MyAppExe}
VersionInfoVersion={#MyAppVersion}
VersionInfoCompany=Ezoo
VersionInfoDescription=Ezoo Download Setup
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoCopyright=Copyright (C) 2026 Ahmed Ezzat
AppPublisherURL=https://www.youtube.com/@Ezoo-Tech
UninstallDisplayName={#MyAppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
#ifndef Legacy
; 64-bit Windows gets the 64-bit program, 32-bit Windows 10 the 32-bit one
; (both are in this setup, see [Files])
ArchitecturesInstallIn64BitMode=x64compatible
#endif
CloseApplications=yes
RestartApplications=no

; the language of the setup is also the language of the program: asked
; once, at the first install (Windows' own language is chosen at first);
; an update keeps it and asks nothing
ShowLanguageDialog=yes
UsePreviousLanguage=yes
LanguageDetectionMethod=uilanguage

[Languages]
; the names are the program's own language codes (see [Registry])
Name: "en"; MessagesFile: "compiler:Default.isl"
#if FileExists(AddBackslash(CompilerPath) + "Languages\Arabic.isl")
Name: "ar"; MessagesFile: "compiler:Languages\Arabic.isl"
#else
Name: "ar"; MessagesFile: "lang\Arabic.isl"
#endif
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "tr"; MessagesFile: "compiler:Languages\Turkish.isl"

[CustomMessages]
en.GroupShortcuts=Shortcuts:
en.TaskDesktop=Create a desktop shortcut
en.GroupStartup=Startup:
en.TaskAutostart=Start Ezoo Download when Windows starts (recommended)
en.GroupTorrents=Torrents:
en.TaskTorrents=Make Ezoo Download the main program for torrents (.torrent files and magnet links)
en.RunNow=Open Ezoo Download now
en.SystemFiles=Installing required system files...
ar.GroupShortcuts=الاختصارات:
ar.TaskDesktop=إنشاء اختصار على سطح المكتب
ar.GroupStartup=بدء التشغيل:
ar.TaskAutostart=تشغيل Ezoo Download مع بدء Windows (مُستحسن)
ar.GroupTorrents=التورنت:
ar.TaskTorrents=جعل Ezoo Download البرنامج الرئيسي للتورنت (ملفات ‎.torrent وروابط magnet)
ar.RunNow=فتح Ezoo Download الآن
ar.SystemFiles=جارٍ تثبيت ملفات النظام المطلوبة...
fr.GroupShortcuts=Raccourcis :
fr.TaskDesktop=Créer un raccourci sur le Bureau
fr.GroupStartup=Démarrage :
fr.TaskAutostart=Lancer Ezoo Download au démarrage de Windows (recommandé)
fr.GroupTorrents=Torrents :
fr.TaskTorrents=Faire d'Ezoo Download le programme principal pour les torrents (fichiers .torrent et liens magnet)
fr.RunNow=Ouvrir Ezoo Download maintenant
fr.SystemFiles=Installation des fichiers système nécessaires...
de.GroupShortcuts=Verknüpfungen:
de.TaskDesktop=Desktop-Verknüpfung erstellen
de.GroupStartup=Systemstart:
de.TaskAutostart=Ezoo Download beim Start von Windows starten (empfohlen)
de.GroupTorrents=Torrents:
de.TaskTorrents=Ezoo Download zum Hauptprogramm für Torrents machen (.torrent-Dateien und Magnet-Links)
de.RunNow=Ezoo Download jetzt öffnen
de.SystemFiles=Benötigte Systemdateien werden installiert...
es.GroupShortcuts=Accesos directos:
es.TaskDesktop=Crear un acceso directo en el escritorio
es.GroupStartup=Inicio:
es.TaskAutostart=Iniciar Ezoo Download al iniciar Windows (recomendado)
es.GroupTorrents=Torrents:
es.TaskTorrents=Hacer de Ezoo Download el programa principal para torrents (archivos .torrent y enlaces magnet)
es.RunNow=Abrir Ezoo Download ahora
es.SystemFiles=Instalando los archivos del sistema necesarios...
pt.GroupShortcuts=Atalhos:
pt.TaskDesktop=Criar um atalho na área de trabalho
pt.GroupStartup=Inicialização:
pt.TaskAutostart=Iniciar o Ezoo Download com o Windows (recomendado)
pt.GroupTorrents=Torrents:
pt.TaskTorrents=Tornar o Ezoo Download o programa principal para torrents (arquivos .torrent e links magnet)
pt.RunNow=Abrir o Ezoo Download agora
pt.SystemFiles=Instalando os arquivos de sistema necessários...
ru.GroupShortcuts=Ярлыки:
ru.TaskDesktop=Создать ярлык на рабочем столе
ru.GroupStartup=Автозапуск:
ru.TaskAutostart=Запускать Ezoo Download вместе с Windows (рекомендуется)
ru.GroupTorrents=Торренты:
ru.TaskTorrents=Сделать Ezoo Download основной программой для торрентов (файлы .torrent и magnet-ссылки)
ru.RunNow=Открыть Ezoo Download сейчас
ru.SystemFiles=Установка необходимых системных файлов...
tr.GroupShortcuts=Kısayollar:
tr.TaskDesktop=Masaüstü kısayolu oluştur
tr.GroupStartup=Başlangıç:
tr.TaskAutostart=Ezoo Download'ı Windows ile başlat (önerilir)
tr.GroupTorrents=Torrentler:
tr.TaskTorrents=Ezoo Download'ı torrentler için ana program yap (.torrent dosyaları ve magnet bağlantıları)
tr.RunNow=Ezoo Download'ı şimdi aç
tr.SystemFiles=Gerekli sistem dosyaları yükleniyor...

[Tasks]
Name: "desktopicon"; Description: "{cm:TaskDesktop}"; GroupDescription: "{cm:GroupShortcuts}"
Name: "autostart"; Description: "{cm:TaskAutostart}"; GroupDescription: "{cm:GroupStartup}"
Name: "torrents"; Description: "{cm:TaskTorrents}"; GroupDescription: "{cm:GroupTorrents}"

[Files]
#ifdef Legacy
Source: "{#Stage}\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
; the helper the browser starts (the program copies it into its data folder)
Source: "{#Stage}\ezoo-host.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
#else
Source: "{#Stage}\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion; Check: Is64BitInstallMode
Source: "{#Stage}\x86\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion; Check: not Is64BitInstallMode
; the helper the browser starts (the program copies it into its data folder)
Source: "{#Stage}\ezoo-host.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist; Check: Is64BitInstallMode
Source: "{#Stage}\x86\ezoo-host.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist; Check: not Is64BitInstallMode
#endif
Source: "{#Stage}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Stage}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; the helpers (our own ffmpeg, the official yt-dlp) go into {app}\tools; the
; program keeps them updated there (and fetches one again if it goes missing)
#ifdef Legacy
; Windows 7 / 8 / 8.1: yt-dlp is its Python build run by our Python (tools\python)
Source: "{#Stage}\tools\*"; DestDir: "{app}\tools"; Flags: ignoreversion skipifsourcedoesntexist recursesubdirs createallsubdirs
#else
Source: "{#Stage}\tools\*"; DestDir: "{app}\tools"; Flags: ignoreversion skipifsourcedoesntexist; Check: Is64BitInstallMode
; 32-bit Windows 10: the 32-bit program, ffmpeg and yt-dlp
Source: "{#Stage}\x86\tools\*"; DestDir: "{app}\tools"; Flags: ignoreversion skipifsourcedoesntexist; Check: not Is64BitInstallMode
#endif
Source: "{#Stage}\extension\*"; DestDir: "{app}\extension"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\extension-firefox\*"; DestDir: "{app}\extension-firefox"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\ezoo-download-firefox.xpi"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; system runtime files, installed only when they are missing (see [Code])
#ifexist "..\build-cache\redist\vc_redist.x64.exe"
  #define HaveRedist
Source: "..\build-cache\redist\vc_redist.x64.exe"; DestDir: "{tmp}"; Flags: dontcopy
#endif

[InstallDelete]
; older versions kept the helpers next to the program
Type: files; Name: "{app}\yt-dlp.exe"
Type: files; Name: "{app}\ffmpeg.exe"
; torrents are shared by the program itself now: the old helper goes
Type: files; Name: "{app}\tools\aria2c.exe"
Type: files; Name: "{app}\tools\aria2-COPYING.txt"
; the browser add-in is put in again whole: nothing of an older version
; stays behind to clash with the new one (the folder is the same, so the
; browser keeps the add-in; the program writes its key in again at start)
Type: filesandordirs; Name: "{app}\extension"
Type: filesandordirs; Name: "{app}\extension-firefox"
; the Firefox add-on of an older version never stays: only this setup's own
; one (or none) is there to be added to Firefox
Type: files; Name: "{app}\ezoo-download-firefox.xpi"

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"
Name: "{group}\Browser extension folder"; Filename: "{app}\extension"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon

[Registry]
; tells the app the installer already decided about "start with Windows"
Root: HKCU; Subkey: "Software\Ezoo\EzooDownload"; ValueType: string; ValueName: "Installed"; ValueData: "1"; Flags: uninsdeletekey
; the first start of the program takes these (its own settings win later)
Root: HKCU; Subkey: "Software\Ezoo\EzooDownload"; ValueType: string; ValueName: "Language"; ValueData: "{language}"
Root: HKCU; Subkey: "Software\Ezoo\EzooDownload"; ValueType: string; ValueName: "Torrents"; ValueData: "1"; Tasks: torrents
Root: HKCU; Subkey: "Software\Ezoo\EzooDownload"; ValueType: string; ValueName: "Torrents"; ValueData: "0"; Tasks: not torrents
; start in the background (next to the clock) when the user signs in
; the choice only: the program itself adds (or not) its "start with Windows"
; on its first start, the setup writes nothing that starts with Windows
Root: HKCU; Subkey: "Software\Ezoo\EzooDownload"; ValueType: string; ValueName: "Autostart"; ValueData: "1"; Tasks: autostart
Root: HKCU; Subkey: "Software\Ezoo\EzooDownload"; ValueType: string; ValueName: "Autostart"; ValueData: "0"; Tasks: not autostart
; removed with the program (the value the program added)
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "EzooDownload"; Flags: uninsdeletevalue dontcreatekey
; the browsers' entry for the program's helper (the program fills it in at
; start): gone again when Ezoo Download is removed
Root: HKCU; Subkey: "Software\Google\Chrome\NativeMessagingHosts\com.ezoo.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Chromium\NativeMessagingHosts\com.ezoo.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Edge\NativeMessagingHosts\com.ezoo.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.ezoo.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Mozilla\NativeMessagingHosts\com.ezoo.download"; ValueType: none; Flags: uninsdeletekey
; "Open Ezoo Download" in the browser add-in (ezoo-download://open). The
; address given by the browser is never passed to the program.
Root: HKCU; Subkey: "Software\Classes\ezoo-download"; ValueType: string; ValueName: ""; ValueData: "URL:Ezoo Download"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\ezoo-download"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\ezoo-download\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExe},0"
Root: HKCU; Subkey: "Software\Classes\ezoo-download\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" --from-browser"

; .torrent files and magnet: links (only when chosen while installing).
; Windows may still ask once which program to use.
Root: HKCU; Subkey: "Software\Classes\.torrent\OpenWithProgids"; ValueType: string; ValueName: "EzooDownload.torrent"; ValueData: ""; Flags: uninsdeletevalue; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueName: ""; ValueData: "EzooDownload.torrent"; Flags: uninsdeletevalue; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\EzooDownload.torrent"; ValueType: string; ValueName: ""; ValueData: "Torrent file"; Flags: uninsdeletekey; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\EzooDownload.torrent\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExe},0"; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\EzooDownload.torrent\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: ""; ValueData: "URL:Magnet link"; Flags: uninsdeletekey; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExe},0"; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Tasks: torrents

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "{cm:RunNow}"; Flags: nowait postinstall skipifsilent
; after an automatic update: start the new version again
Filename: "{app}\{#MyAppExe}"; Flags: nowait; Check: IsAutoUpdate


[UninstallRun]

[UninstallDelete]
Type: filesandordirs; Name: "{app}\extension"
Type: filesandordirs; Name: "{app}\extension-firefox"
Type: filesandordirs; Name: "{app}\tools"
Type: filesandordirs; Name: "{userappdata}\Ezoo\EzooDownload\data\host"

[Code]
function IsAutoUpdate: Boolean;
begin
  Result := ExpandConstant('{param:AUTOUPDATE|0}') = '1';
end;

{ the Visual C++ 2015-2022 runtime (vcruntime140.dll) }
function RuntimeMissing: Boolean;
var
  Installed: Cardinal;
begin
  Result := True;
  if RegQueryDWordValue(HKLM64, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Installed', Installed) and (Installed = 1)
     and FileExists(ExpandConstant('{sys}\vcruntime140.dll'))
     and FileExists(ExpandConstant('{sys}\vcruntime140_1.dll'))
     and FileExists(ExpandConstant('{sys}\msvcp140.dll')) then
    Result := False;
end;

procedure InstallRuntime;
var
  Code: Integer;
begin
#ifdef HaveRedist
  { the 64-bit runtime, for the 64-bit program only }
  if not Is64BitInstallMode then exit;
  if not RuntimeMissing then exit;
  { an automatic update must not show any window }
  if WizardSilent then exit;
  try
    ExtractTemporaryFile('vc_redist.x64.exe');
  except
    exit;
  end;
  WizardForm.StatusLabel.Caption := CustomMessage('SystemFiles');
  { needs administrator rights: Windows asks once. If that is refused the
    program still works, it carries its own copy of these files. }
  ShellExec('runas', ExpandConstant('{tmp}\vc_redist.x64.exe'), '/install /quiet /norestart', '', SW_HIDE, ewWaitUntilTerminated, Code);
#endif
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    InstallRuntime;
  end;
end;

{ true while EzooDownload.exe still runs (it keeps this mark while open) }
function AppRunning: Boolean;
begin
  Result := CheckForMutexes('EzooDownload.Running');
end;

{ ask the open program to close by itself: it saves the list and the
  downloads (they go on after the update). Versions before 1.0.8 don't
  know this; they are closed below. }
procedure AskAppToQuit;
var
  Http: Variant;
begin
  try
    Http := CreateOleObject('WinHttp.WinHttpRequest.5.1');
    Http.SetTimeouts(1000, 1000, 2000, 2000);
    Http.Open('POST', 'http://127.0.0.1:9614/quit', False);
    Http.SetRequestHeader('X-SwiftGet', '1');
    Http.Send('');
  except
  end;
end;

{ the running program locks its files: it is asked to close by itself (it
  saves the list and the downloads); one that does not is closed by
  Windows' own Restart Manager (CloseApplications above) }
procedure CloseApp;
var
  I: Integer;
begin
  AskAppToQuit;
  for I := 1 to 30 do
  begin
    if not AppRunning then break;
    Sleep(500);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseApp;
  Result := '';
end;

function InitializeUninstall: Boolean;
begin
  CloseApp;
  Result := True;
end;
