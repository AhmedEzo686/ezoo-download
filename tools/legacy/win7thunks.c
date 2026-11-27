/* Windows 7 / 8 / 8.1 build of Ezoo Download (tools/legacy/build.sh).
 *
 * A few libraries the program uses (Slint's window code, the `windows`
 * crate) call Windows functions that came with Windows 8 or 10. Windows 7
 * refuses to start a program that names a function its DLLs lack, even one
 * it never reaches. This object is linked first: it answers those names
 * itself, so the program imports none of them.
 *
 *  - SystemParametersInfoForDpi (Windows 10): the real one when Windows
 *    has it, else SystemParametersInfoW with the font sizes scaled to the
 *    asked DPI.
 *  - CoTaskMemFree, CoCreateFreeThreadedMarshaler from combase.dll
 *    (Windows 8): the same functions of ole32.dll, there since Windows 2000.
 *  - RoOriginateErrorW (Windows 8, only adds details to an error): the
 *    real one when Windows has it, else nothing to add.
 *
 * Every name is given twice: the `__imp_` pointer the Rust code calls
 * through, and the function itself.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <objbase.h>

/* ---------------------------------------------------------- user32 */

typedef BOOL(WINAPI *spi_for_dpi_fn)(UINT, UINT, PVOID, UINT, UINT);

static int scale(int v, UINT to, int from) { return MulDiv(v, (int)to, from); }

BOOL WINAPI ezoo_spi_for_dpi(UINT action, UINT param, PVOID pv, UINT win_ini, UINT dpi)
    __asm__("SystemParametersInfoForDpi");
BOOL WINAPI ezoo_spi_for_dpi(UINT action, UINT param, PVOID pv, UINT win_ini, UINT dpi) {
    static spi_for_dpi_fn real;
    static volatile LONG looked;
    if (!looked) {
        real = (spi_for_dpi_fn)(void *)GetProcAddress(GetModuleHandleW(L"user32.dll"), "SystemParametersInfoForDpi");
        InterlockedExchange(&looked, 1);
    }
    if (real) return real(action, param, pv, win_ini, dpi);
    BOOL ok = SystemParametersInfoW(action, param, pv, win_ini);
    if (ok && pv && action == SPI_GETNONCLIENTMETRICS) {
        /* sizes come at the system's DPI: the caller asked for `dpi` */
        HDC dc = GetDC(NULL);
        int sys = dc ? GetDeviceCaps(dc, LOGPIXELSY) : 96;
        if (dc) ReleaseDC(NULL, dc);
        if (sys > 0 && (UINT)sys != dpi) {
            NONCLIENTMETRICSW *m = (NONCLIENTMETRICSW *)pv;
            m->lfCaptionFont.lfHeight = scale(m->lfCaptionFont.lfHeight, dpi, sys);
            m->lfSmCaptionFont.lfHeight = scale(m->lfSmCaptionFont.lfHeight, dpi, sys);
            m->lfMenuFont.lfHeight = scale(m->lfMenuFont.lfHeight, dpi, sys);
            m->lfStatusFont.lfHeight = scale(m->lfStatusFont.lfHeight, dpi, sys);
            m->lfMessageFont.lfHeight = scale(m->lfMessageFont.lfHeight, dpi, sys);
        }
    }
    return ok;
}
void *ezoo_imp_spi_for_dpi __asm__("__imp_SystemParametersInfoForDpi") = (void *)ezoo_spi_for_dpi;

/* ---------------------------------------------------------- combase */

void WINAPI ezoo_task_mem_free(LPVOID p) __asm__("CoTaskMemFree");
void WINAPI ezoo_task_mem_free(LPVOID p) { CoTaskMemFree(p); }
void *ezoo_imp_task_mem_free __asm__("__imp_CoTaskMemFree") = (void *)ezoo_task_mem_free;

HRESULT WINAPI ezoo_ftm(LPUNKNOWN outer, LPUNKNOWN *marshal) __asm__("CoCreateFreeThreadedMarshaler");
HRESULT WINAPI ezoo_ftm(LPUNKNOWN outer, LPUNKNOWN *marshal) { return CoCreateFreeThreadedMarshaler(outer, marshal); }
void *ezoo_imp_ftm __asm__("__imp_CoCreateFreeThreadedMarshaler") = (void *)ezoo_ftm;

/* ---------------------------------------------------------- WinRT errors */

typedef BOOL(WINAPI *ro_originate_fn)(HRESULT, UINT, LPCWSTR);

BOOL WINAPI ezoo_ro_originate(HRESULT error, UINT len, LPCWSTR message) __asm__("RoOriginateErrorW");
BOOL WINAPI ezoo_ro_originate(HRESULT error, UINT len, LPCWSTR message) {
    static ro_originate_fn real;
    static volatile LONG looked;
    if (!looked) {
        /* Windows 8 and newer (never loaded where it does not exist) */
        HMODULE m = LoadLibraryExW(L"combase.dll", NULL, 0x00000800 /* LOAD_LIBRARY_SEARCH_SYSTEM32 */);
        if (!m) m = LoadLibraryW(L"combase.dll");
        real = m ? (ro_originate_fn)(void *)GetProcAddress(m, "RoOriginateErrorW") : NULL;
        InterlockedExchange(&looked, 1);
    }
    return real ? real(error, len, message) : FALSE;
}
void *ezoo_imp_ro_originate __asm__("__imp_RoOriginateErrorW") = (void *)ezoo_ro_originate;

/* ---------------------------------------------------------- crash note */

/* A crash outside Rust's own checks (a bad pointer, code where there is
 * none) leaves no crash.log: Windows only says "has stopped working". This
 * note, written by a handler set before main, says where it happened: the
 * address and the program's own addresses found on the stack, relative to
 * the program (the build keeps a copy with names to read them).
 * %APPDATA%\Ezoo\EzooDownload\data\crash-native.log, a few lines a run. */
static volatile LONG notes;

static void note_hex(char **p, const char *label, DWORD v) {
    static const char hex[] = "0123456789abcdef";
    while (*label) *(*p)++ = *label++;
    for (int i = 7; i >= 0; i--) *(*p)++ = hex[(v >> (i * 4)) & 15];
}

static LONG CALLBACK ezoo_crash_note(EXCEPTION_POINTERS *ep) {
    DWORD code = ep->ExceptionRecord->ExceptionCode;
    if (code != EXCEPTION_ACCESS_VIOLATION && code != EXCEPTION_ILLEGAL_INSTRUCTION &&
        code != EXCEPTION_STACK_OVERFLOW && code != EXCEPTION_PRIV_INSTRUCTION &&
        code != 0xC0000409 /* stack buffer overrun / fail fast */)
        return EXCEPTION_CONTINUE_SEARCH;
    if (InterlockedIncrement(&notes) > 4) return EXCEPTION_CONTINUE_SEARCH;
    HMODULE exe = GetModuleHandleW(NULL);
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)((BYTE *)exe + ((IMAGE_DOS_HEADER *)exe)->e_lfanew);
    DWORD base = (DWORD)(ULONG_PTR)exe, end = base + nt->OptionalHeader.SizeOfImage;
    char buf[1024], *p = buf;
    note_hex(&p, "code ", code);
    note_hex(&p, " at ", (DWORD)(ULONG_PTR)ep->ExceptionRecord->ExceptionAddress);
    if (ep->ExceptionRecord->NumberParameters >= 2) {
        note_hex(&p, " kind ", (DWORD)ep->ExceptionRecord->ExceptionInformation[0]);
        note_hex(&p, " addr ", (DWORD)ep->ExceptionRecord->ExceptionInformation[1]);
    }
    note_hex(&p, " base ", base);
#ifdef _M_IX86
    note_hex(&p, " eip ", ep->ContextRecord->Eip);
    DWORD *sp = (DWORD *)(ULONG_PTR)ep->ContextRecord->Esp;
    const char *l = " | stack:";
    while (*l) *p++ = *l++;
    /* the program's own return addresses near the top of the stack */
    for (int i = 0, n = 0; i < 2048 && n < 40 && p < buf + sizeof(buf) - 16; i++) {
        DWORD v;
        if (IsBadReadPtr(sp + i, 4)) break;
        v = sp[i];
        if (v > base + 0x1000 && v < end) { note_hex(&p, " +", v - base); n++; }
    }
#endif
    *p++ = '\r'; *p++ = '\n';
    WCHAR path[MAX_PATH + 64];
    DWORD len = GetEnvironmentVariableW(L"APPDATA", path, MAX_PATH);
    if (len && len < MAX_PATH) {
        lstrcatW(path, L"\\Ezoo\\EzooDownload\\data\\crash-native.log");
        HANDLE f = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_ALWAYS, 0, NULL);
        if (f != INVALID_HANDLE_VALUE) {
            DWORD w;
            WriteFile(f, buf, (DWORD)(p - buf), &w, NULL);
            CloseHandle(f);
        }
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

__attribute__((constructor)) static void ezoo_crash_note_install(void) {
    AddVectoredExceptionHandler(0, ezoo_crash_note);
}
