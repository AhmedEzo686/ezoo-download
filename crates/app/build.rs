fn main() {
    // "fluent" follows the light/dark switch the app makes at runtime
    // Arabic texts: lang/ar/LC_MESSAGES/swiftget.po (made by lang/make_po.py)
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    for lang in ["ar", "fr", "es", "de", "pt", "tr", "ru"] {
        println!("cargo:rerun-if-changed=lang/{lang}/LC_MESSAGES/swiftget.po");
    }
    println!("cargo:rerun-if-changed=lang/rs");
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint UI failed to compile");

    // Icon, full version information and a standard manifest for
    // EzooDownload.exe. Antivirus programs trust a program more when it says
    // clearly who made it, what it is and that it runs without admin rights.
    #[cfg(windows)]
    {
        let version = env!("CARGO_PKG_VERSION");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("ProductName", "Ezoo Download");
        res.set("FileDescription", "Ezoo Download");
        res.set("CompanyName", "Ezoo");
        res.set("LegalCopyright", "Copyright (C) 2026 Ahmed Ezzat");
        res.set("OriginalFilename", "EzooDownload.exe");
        res.set("InternalName", "EzooDownload");
        res.set("ProductVersion", version);
        res.set("FileVersion", version);
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="Ezoo.EzooDownload" version="1.0.0.0" processorArchitecture="*"/>
  <description>Ezoo Download - download manager</description>
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
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#,
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
