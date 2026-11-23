fn main() {
    // Version information and a standard manifest for ezoo-host.exe, like
    // EzooDownload.exe has: antivirus programs trust a program more when it
    // says who made it and what it is.
    // the program being built is for Windows (also when built on Linux
    // for Windows 7, tools/legacy)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = env!("CARGO_PKG_VERSION");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../app/icon.ico");
        res.set("ProductName", "Ezoo Download");
        res.set("FileDescription", "Ezoo Download browser helper");
        res.set("CompanyName", "Ezoo");
        res.set("LegalCopyright", "Copyright (C) 2026 Ahmed Ezzat");
        res.set("OriginalFilename", "ezoo-host.exe");
        res.set("InternalName", "ezoo-host");
        res.set("ProductVersion", version);
        res.set("FileVersion", version);
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="Ezoo.EzooDownload.Host" version="1.0.0.0" processorArchitecture="*"/>
  <description>Ezoo Download - the helper a browser starts to talk to the program</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 7, 8, 8.1 (the legacy build), 10 and 11 -->
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#,
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the version information: {e}");
        }
    }
}
