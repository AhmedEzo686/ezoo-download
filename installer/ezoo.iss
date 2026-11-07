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
OutputBaseFilename=EzooDownload-Setup
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
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
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
en.TaskFirewall=Let torrents through the Windows firewall (faster torrents; Windows asks for permission once)
en.RunNow=Open Ezoo Download now
en.SystemFiles=Installing required system files...
ar.GroupShortcuts=الاختصارات:
ar.TaskDesktop=إنشاء اختصار على سطح المكتب
ar.GroupStartup=بدء التشغيل:
ar.TaskAutostart=تشغيل Ezoo Download مع بدء Windows (مُستحسن)
ar.GroupTorrents=التورنت:
ar.TaskTorrents=جعل Ezoo Download البرنامج الرئيسي للتورنت (ملفات ‎.torrent وروابط magnet)
ar.TaskFirewall=السماح للتورنت بالمرور من جدار حماية Windows (تورنت أسرع؛ يطلب Windows الإذن مرة واحدة)
ar.RunNow=فتح Ezoo Download الآن
ar.SystemFiles=جارٍ تثبيت ملفات النظام المطلوبة...
fr.GroupShortcuts=Raccourcis :
fr.TaskDesktop=Créer un raccourci sur le Bureau
fr.GroupStartup=Démarrage :
fr.TaskAutostart=Lancer Ezoo Download au démarrage de Windows (recommandé)
fr.GroupTorrents=Torrents :
fr.TaskTorrents=Faire d'Ezoo Download le programme principal pour les torrents (fichiers .torrent et liens magnet)
fr.TaskFirewall=Laisser passer les torrents dans le pare-feu Windows (torrents plus rapides ; Windows demande l'autorisation une fois)
fr.RunNow=Ouvrir Ezoo Download maintenant
fr.SystemFiles=Installation des fichiers système nécessaires...
de.GroupShortcuts=Verknüpfungen:
de.TaskDesktop=Desktop-Verknüpfung erstellen
de.GroupStartup=Systemstart:
de.TaskAutostart=Ezoo Download beim Start von Windows starten (empfohlen)
de.GroupTorrents=Torrents:
de.TaskTorrents=Ezoo Download zum Hauptprogramm für Torrents machen (.torrent-Dateien und Magnet-Links)
de.TaskFirewall=Torrents durch die Windows-Firewall lassen (schnellere Torrents; Windows fragt einmal um Erlaubnis)
de.RunNow=Ezoo Download jetzt öffnen
de.SystemFiles=Benötigte Systemdateien werden installiert...
es.GroupShortcuts=Accesos directos:
es.TaskDesktop=Crear un acceso directo en el escritorio
es.GroupStartup=Inicio:
es.TaskAutostart=Iniciar Ezoo Download al iniciar Windows (recomendado)
es.GroupTorrents=Torrents:
es.TaskTorrents=Hacer de Ezoo Download el programa principal para torrents (archivos .torrent y enlaces magnet)
es.TaskFirewall=Dejar pasar los torrents por el firewall de Windows (torrents más rápidos; Windows pide permiso una vez)
es.RunNow=Abrir Ezoo Download ahora
es.SystemFiles=Instalando los archivos del sistema necesarios...
pt.GroupShortcuts=Atalhos:
pt.TaskDesktop=Criar um atalho na área de trabalho
pt.GroupStartup=Inicialização:
pt.TaskAutostart=Iniciar o Ezoo Download com o Windows (recomendado)
pt.GroupTorrents=Torrents:
pt.TaskTorrents=Tornar o Ezoo Download o programa principal para torrents (arquivos .torrent e links magnet)
pt.TaskFirewall=Liberar os torrents no firewall do Windows (torrents mais rápidos; o Windows pede permissão uma vez)
pt.RunNow=Abrir o Ezoo Download agora
pt.SystemFiles=Instalando os arquivos de sistema necessários...
ru.GroupShortcuts=Ярлыки:
ru.TaskDesktop=Создать ярлык на рабочем столе
ru.GroupStartup=Автозапуск:
ru.TaskAutostart=Запускать Ezoo Download вместе с Windows (рекомендуется)
ru.GroupTorrents=Торренты:
ru.TaskTorrents=Сделать Ezoo Download основной программой для торрентов (файлы .torrent и magnet-ссылки)
ru.TaskFirewall=Пропускать торренты через брандмауэр Windows (торренты быстрее; Windows один раз спросит разрешение)
ru.RunNow=Открыть Ezoo Download сейчас
ru.SystemFiles=Установка необходимых системных файлов...
tr.GroupShortcuts=Kısayollar:
tr.TaskDesktop=Masaüstü kısayolu oluştur
tr.GroupStartup=Başlangıç:
tr.TaskAutostart=Ezoo Download'ı Windows ile başlat (önerilir)
tr.GroupTorrents=Torrentler:
tr.TaskTorrents=Ezoo Download'ı torrentler için ana program yap (.torrent dosyaları ve magnet bağlantıları)
tr.TaskFirewall=Torrentlerin Windows güvenlik duvarından geçmesine izin ver (daha hızlı torrentler; Windows bir kez izin ister)
tr.RunNow=Ezoo Download'ı şimdi aç
tr.SystemFiles=Gerekli sistem dosyaları yükleniyor...

[Tasks]
Name: "desktopicon"; Description: "{cm:TaskDesktop}"; GroupDescription: "{cm:GroupShortcuts}"
Name: "autostart"; Description: "{cm:TaskAutostart}"; GroupDescription: "{cm:GroupStartup}"
Name: "torrents"; Description: "{cm:TaskTorrents}"; GroupDescription: "{cm:GroupTorrents}"
Name: "firewall"; Description: "{cm:TaskFirewall}"; GroupDescription: "{cm:GroupTorrents}"

[Files]
Source: "{#Stage}\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
; the helper the browser starts (the program copies it into its data folder)
Source: "{#Stage}\ezoo-host.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Stage}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Stage}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; the helpers go into {app}\tools; the app updates them there by itself
Source: "{#Stage}\tools\*"; DestDir: "{app}\tools"; Flags: ignoreversion skipifsourcedoesntexist
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
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "EzooDownload"; ValueData: """{app}\{#MyAppExe}"" --background"; Flags: uninsdeletevalue; Tasks: autostart
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
Filename: "{cmd}"; Parameters: "/C taskkill /IM {#MyAppExe} /F"; Flags: runhidden; RunOnceId: "StopEzoo"
Filename: "{cmd}"; Parameters: "/C taskkill /IM ezoo-host-* /F"; Flags: runhidden; RunOnceId: "StopEzooHost"

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

{ the torrent helper may take people's connections: Windows' firewall lets
  it (needs administrator rights: Windows asks once; refused = torrents still
  work, a little slower) }
procedure AllowTorrents;
var
  Code: Integer;
  Tool: String;
begin
  if WizardSilent then exit;
  Tool := ExpandConstant('{app}\tools\aria2c.exe');
  ShellExec('runas', ExpandConstant('{sys}\cmd.exe'),
    '/C netsh advfirewall firewall delete rule name="Ezoo Download torrents" >nul 2>&1 & ' +
    'netsh advfirewall firewall add rule name="Ezoo Download torrents" dir=in action=allow program="' + Tool + '" enable=yes profile=any >nul 2>&1',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    InstallRuntime;
    if WizardIsTaskSelected('firewall') then
      AllowTorrents;
  end;
end;

{ true while EzooDownload.exe still runs }
function AppRunning: Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'), '/C tasklist /NH /FI "IMAGENAME eq {#MyAppExe}" | find /I "{#MyAppExe}" >nul', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
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

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code, I: Integer;
begin
  { the running program locks its files: close it before copying }
  if AppRunning then
  begin
    AskAppToQuit;
    for I := 1 to 20 do
    begin
      if not AppRunning then break;
      Sleep(500);
    end;
  end;
  if AppRunning then
  begin
    Exec(ExpandConstant('{cmd}'), '/C taskkill /IM {#MyAppExe} >nul 2>&1', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Sleep(2000);
  end;
  if AppRunning then
  begin
    Exec(ExpandConstant('{cmd}'), '/C taskkill /F /IM {#MyAppExe} >nul 2>&1', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Sleep(800);
  end;
  Result := '';
end;
