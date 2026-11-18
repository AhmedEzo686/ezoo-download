fn main() {
    // Version information and a standard manifest for ezoo-host.exe, like
    // EzooDownload.exe has: antivirus programs trust a program more when it
    // says who made it and what it is.
    #[cfg(windows)]
    {
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
