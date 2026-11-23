#!/usr/bin/env python3
"""win7check.py [--ucrt-ok] file.exe|.dll ...

Does Windows 7 SP1 have everything these programs ask of Windows when they
start? Lists every DLL a file imports that Windows 7 lacks (unless it is
next to the file) and every function newer than Windows 7
(newer-than-win7.txt). Exit code 1 when something is missing: Windows 7
would refuse to start the program.

--ucrt-ok: the Universal C Runtime (api-ms-win-crt-*.dll, KB2999226) is
fine, the program offers that update itself (Python)."""
import os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
newer = {}
for line in open(os.path.join(HERE, "newer-than-win7.txt")):
    if line.strip() and not line.startswith("#"):
        dll, fn, ver = line.split()
        newer[fn] = ver
WIN7_DLLS = set("""advapi32 kernel32 user32 gdi32 shell32 ole32 oleaut32 comctl32 comdlg32 ws2_32 wsock32
msvcrt ntdll crypt32 bcrypt ncrypt secur32 shlwapi uxtheme dwmapi winmm version imm32 userenv iphlpapi
winhttp wininet opengl32 propsys dbghelp psapi setupapi rpcrt4 dnsapi oleacc winspool.drv d2d1 dwrite
d3d11 d3d9 dxgi windowscodecs msimg32 usp10 powrprof normaliz netapi32 mpr credui wtsapi32 cfgmgr32 hid
winusb mswsock uiautomationcore shfolder urlmon wintrust imagehlp gdiplus glu32 cryptnet api-ms-win-core-synch-l1-1-0
api-ms-win-core-synch-l1-2-0x""".split()) - {"api-ms-win-core-synch-l1-2-0x"}
args = sys.argv[1:]
ucrt_ok = "--ucrt-ok" in args
files = [a for a in args if a != "--ucrt-ok"]
bad = 0
for f in files:
    here = {n.lower().removesuffix(".dll") for n in os.listdir(os.path.dirname(os.path.abspath(f)))}
    out = subprocess.run(["llvm-objdump", "-p", f], capture_output=True, text=True, check=True).stdout
    dll, probs = None, []
    for line in out.splitlines():
        m = re.match(r"\s+DLL Name: (\S+)", line)
        if m:
            dll = m.group(1).lower().removesuffix(".dll")
            if dll in WIN7_DLLS or dll in here or (ucrt_ok and dll.startswith("api-ms-win-crt-")):
                continue
            probs.append(f"needs {dll}.dll (not on Windows 7)")
            continue
        m = re.match(r"\s+\d+\s+(\w+)$", line)
        if m and dll and dll not in here and m.group(1) in newer:
            probs.append(f"needs {dll}!{m.group(1)} (Windows {newer[m.group(1)]})")
    print(f"{os.path.basename(f)}: {'Windows 7 OK' if not probs else 'NOT for Windows 7'}")
    for p in probs:
        print("   ", p)
    bad |= bool(probs)
sys.exit(1 if bad else 0)
