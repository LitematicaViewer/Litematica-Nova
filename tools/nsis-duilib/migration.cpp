#include "nsis_abi.h"
#include <shellapi.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <msi.h>
#include <tlhelp32.h>
#include <psapi.h>
#include <wincrypt.h>
#include <algorithm>
#include <array>
#include <set>
#include <string_view>
#include <vector>

namespace {
constexpr wchar_t identity[] = L"com.litematica.nova";
constexpr wchar_t uninstallRoot[] = L"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
constexpr wchar_t stableUpgradeCode[] = L"{30D2825F-22C4-594D-A998-8BBEDE2E81D3}";
const std::array<std::wstring, 3> names{L"Litematica Nova", L"Litematica-Nova", L"Litematica Nova BA"};
bool equal(const std::wstring& a, const std::wstring& b) { return _wcsicmp(a.c_str(), b.c_str()) == 0; }
bool knownName(const std::wstring& name) { return std::any_of(names.begin(), names.end(), [&](const auto& n) { return equal(n, name); }); }
std::wstring lower(std::wstring text) { std::transform(text.begin(), text.end(), text.begin(), towlower); return text; }
struct Key {
    HKEY value = nullptr;
    Key() = default;
    Key(HKEY root, const std::wstring& path, REGSAM access) { RegOpenKeyExW(root, path.c_str(), 0, access, &value); }
    ~Key() { if (value) RegCloseKey(value); }
    Key(const Key&) = delete;
    Key& operator=(const Key&) = delete;
};
std::wstring read(HKEY key, const wchar_t* name) {
    DWORD type = 0, size = 0;
    if (!key || RegQueryValueExW(key, name, nullptr, &type, nullptr, &size) != ERROR_SUCCESS ||
            (type != REG_SZ && type != REG_EXPAND_SZ) || size > 131072) return {};
    std::wstring result(size / sizeof(wchar_t) + 1, L'\0');
    if (RegQueryValueExW(key, name, nullptr, &type, reinterpret_cast<BYTE*>(result.data()), &size) != ERROR_SUCCESS) return {};
    result.resize(wcsnlen_s(result.c_str(), result.size()));
    return result;
}
bool driveRootAt(std::wstring_view path, size_t at) {
    if (path.size() < at + 3) return false;
    const auto drive = path[at];
    return ((drive >= L'A' && drive <= L'Z') || (drive >= L'a' && drive <= L'z')) &&
        path[at + 1] == L':' && path[at + 2] == L'\\';
}
// A normal Win32 path trims dots/spaces and resolves device names. Do not
// turn a verbatim component into a different file in the editable directory page.
bool ordinaryComponent(std::wstring_view component) {
    if (component.empty() || component.size() > 255 || component.back() == L'.' || component.back() == L' ' ||
        component.find_first_of(L"<>:\"/|?*") != component.npos ||
        std::any_of(component.begin(), component.end(), [](wchar_t c) { return c < L' '; })) return false;
    auto stem = component.substr(0, component.find(L'.'));
    while (!stem.empty() && stem.back() == L' ') stem.remove_suffix(1);
    const auto name = lower(std::wstring(stem));
    if (name == L"con" || name == L"prn" || name == L"aux" || name == L"nul" ||
        name == L"conin$" || name == L"conout$") return false;
    return !(name.size() == 4 && (name.compare(0, 3, L"com") == 0 || name.compare(0, 3, L"lpt") == 0) &&
        std::wstring_view(L"123456789\u00b9\u00b2\u00b3").find(name[3]) != std::wstring_view::npos);
}
std::wstring ordinaryPath(const std::wstring& path) {
    if (path.compare(0, 4, L"\\\\?\\") != 0) return path;
    const bool unc = _wcsnicmp(path.c_str() + 4, L"UNC\\", 4) == 0;
    if (!unc && !driveRootAt(path, 4)) return path; // Volume GUIDs and other device namespaces.
    const std::wstring_view view(path);
    size_t count = 0;
    for (size_t at = unc ? 8 : 7; at < view.size(); ++count) {
        const auto end = view.find(L'\\', at);
        if (!ordinaryComponent(view.substr(at, end - at))) return path;
        at = end == view.npos ? view.size() : end + 1;
    }
    if (unc && count < 2) return path; // Both server and share are required.
    return unc ? L"\\\\" + path.substr(8) : path.substr(4);
}
bool sameInstallPath(const std::wstring& a, const std::wstring& b) {
    return equal(a, b) || equal(ordinaryPath(a), ordinaryPath(b));
}
// Short, losslessly representable paths are shared by the directory page and
// Windows Shell. Keep verbatim-only names and genuine long paths intact.
std::wstring shellPath(const std::wstring& path) {
    auto ordinary = ordinaryPath(path);
    return ordinary.size() < MAX_PATH ? ordinary : path;
}
std::wstring normalize(std::wstring path) {
    if (path.size() > 1 && path.front() == L'"' && path.back() == L'"') path = path.substr(1, path.size() - 2);
    const size_t root = path.compare(0, 4, L"\\\\?\\") == 0 && driveRootAt(path, 4) ? 7 : 3;
    while (path.size() > root && (path.back() == L'\\' || path.back() == L'/')) path.pop_back();
    return path;
}
bool exists(const std::wstring& path) { const auto a = GetFileAttributesW(path.c_str()); return a != INVALID_FILE_ATTRIBUTES && !(a & FILE_ATTRIBUTE_DIRECTORY); }
std::wstring versionName(const std::wstring& file) {
    DWORD unused = 0, size = GetFileVersionInfoSizeW(file.c_str(), &unused);
    if (!size || size > 1048576) return {};
    std::vector<BYTE> data(size);
    if (!GetFileVersionInfoW(file.c_str(), 0, size, data.data())) return {};
    struct Translation { WORD language, page; };
    Translation* translations = nullptr; UINT bytes = 0;
    if (!VerQueryValueW(data.data(), L"\\VarFileInfo\\Translation", reinterpret_cast<void**>(&translations), &bytes)) return {};
    for (UINT i = 0; i < bytes / sizeof(Translation); ++i) {
        wchar_t query[80]{};
        swprintf_s(query, L"\\StringFileInfo\\%04x%04x\\ProductName", translations[i].language, translations[i].page);
        wchar_t* value = nullptr; UINT count = 0;
        if (VerQueryValueW(data.data(), query, reinterpret_cast<void**>(&value), &count) && count && knownName(value)) return value;
    }
    return {};
}
std::wstring firstArgument(const std::wstring& command) {
    int count = 0;
    LPWSTR* argv = CommandLineToArgvW(command.c_str(), &count);
    std::wstring result = argv && count ? argv[0] : L"";
    if (argv) LocalFree(argv);
    return result;
}
struct Record {
    HKEY root;
    REGSAM view;
    std::wstring key, name, directory, uninstaller, binary, product;
    bool stable = false;
};
std::vector<Record> migrationRecords;
std::set<std::wstring> relatedMsi() {
    std::set<std::wstring> related;
    wchar_t code[39]{};
    for (DWORD index = 0; MsiEnumRelatedProductsW(stableUpgradeCode, 0, index, code) == ERROR_SUCCESS; ++index) related.insert(lower(code));
    return related;
}
std::vector<Record> records() {
    std::vector<Record> all;
    const auto related = relatedMsi();
    for (HKEY hive : {HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER}) for (REGSAM view : {KEY_WOW64_64KEY, KEY_WOW64_32KEY}) {
        Key root(hive, uninstallRoot, KEY_READ | view);
        if (!root.value) continue;
        for (DWORD index = 0;; ++index) {
            wchar_t subkey[256]{}; DWORD size = 256;
            if (RegEnumKeyExW(root.value, index, subkey, &size, nullptr, nullptr, nullptr, nullptr) != ERROR_SUCCESS) break;
            Record record{hive, view, std::wstring(uninstallRoot) + L"\\" + subkey};
            Key key(hive, record.key, KEY_READ | view);
            record.name = read(key.value, L"DisplayName");
            record.stable = hive == HKEY_LOCAL_MACHINE && view == KEY_WOW64_64KEY &&
                equal(subkey, identity) && equal(read(key.value, L"NovaInstallerIdentity"), identity);
            const bool relatedProduct = related.count(lower(subkey)) != 0;
            if (!record.stable && !relatedProduct && !(knownName(record.name) && equal(read(key.value, L"Publisher"), L"Litematica-Nova"))) continue;
            record.directory = normalize(read(key.value, L"InstallLocation"));
            record.uninstaller = normalize(firstArgument(read(key.value, L"UninstallString")));
            if (record.directory.empty() && !record.uninstaller.empty() && equal(PathFindFileNameW(record.uninstaller.c_str()), L"uninstall.exe")) {
                record.directory = record.uninstaller.substr(0, record.uninstaller.find_last_of(L"\\/"));
            }
            for (const auto& file : {L"Litematica-Nova.exe", L"Litematica Nova.exe", L"Litematica Nova BA.exe"}) {
                const std::wstring candidate = record.directory + L"\\" + file;
                if (exists(candidate) && knownName(versionName(candidate))) { record.binary = candidate; break; }
            }
            GUID guid{};
            const bool productCode = SUCCEEDED(CLSIDFromString(subkey, &guid));
            if (relatedProduct || (productCode && knownName(record.name) && equal(read(key.value, L"Publisher"), L"Litematica-Nova"))) {
                wchar_t productName[512]{}; DWORD length = 512;
                if (relatedProduct || (MsiGetProductInfoW(subkey, INSTALLPROPERTY_PRODUCTNAME, productName, &length) == ERROR_SUCCESS && knownName(productName))) record.product = subkey;
            }
            // A known ARP record without either payload is a stale record, not
            // permission to execute arbitrary UninstallString command lines.
            const bool noPayload = record.binary.empty() && !exists(record.uninstaller);
            const bool trustedUninstaller = sameInstallPath(record.uninstaller, record.directory + L"\\uninstall.exe") && knownName(versionName(record.uninstaller));
            if (record.stable || !record.product.empty() || noPayload || (!record.binary.empty() && trustedUninstaller)) all.push_back(std::move(record));
        }
    }
    std::stable_sort(all.begin(), all.end(), [](const auto& a, const auto& b) { return a.stable && !b.stable; });
    return all;
}
std::wstring processUsing(const std::vector<Record>& entries, const std::wstring& target, bool legacy, bool updateHandoff = false) {
    const ULONGLONG handoffDeadline = GetTickCount64() + 5000;
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return novaText(L"进程状态读取失败，请稍后重试", L"Could not read running processes. Please try again later.");
    PROCESSENTRY32W process{}; process.dwSize = sizeof(process);
    std::wstring running;
    for (BOOL more = Process32FirstW(snapshot, &process); more; more = Process32NextW(snapshot, &process)) {
        if (!equal(process.szExeFile, L"Litematica-Nova.exe") && !equal(process.szExeFile, L"Litematica Nova.exe") && !equal(process.szExeFile, L"Litematica Nova BA.exe")) continue;
        HANDLE handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | (updateHandoff ? SYNCHRONIZE : 0), FALSE, process.th32ProcessID);
        wchar_t buffer[32768]{}; DWORD length = 32768;
        bool relevant = legacy; // Historical uninstallers terminate by name; never invoke one while any Nova is running.
        if (handle && QueryFullProcessImageNameW(handle, 0, buffer, &length)) {
            const std::wstring file(buffer, length);
            const bool destinationProcess = sameInstallPath(file, target + L"\\Litematica-Nova.exe");
            relevant = relevant || destinationProcess;
            for (const auto& r : entries) relevant = relevant || (!r.binary.empty() && sameInstallPath(file, r.binary));
            // The updater starts Setup before its own process exits. Wait only
            // for that same installation's Host to release files; never kill
            // by name and never wait for an unrelated running Nova instance.
            if (updateHandoff && destinationProcess) {
                const auto now = GetTickCount64();
                if (WaitForSingleObject(handle, static_cast<DWORD>(now < handoffDeadline ? handoffDeadline - now : 0)) == WAIT_OBJECT_0) {
                    CloseHandle(handle);
                    continue;
                }
            }
            if (relevant) running = std::wstring(novaText(L"请关闭正在运行的 Litematica-Nova 后重试：", L"Close the running Litematica-Nova and try again: ")) + file;
        } else running = novaText(L"请关闭正在运行的 Litematica-Nova 后重试", L"Close the running Litematica-Nova and try again.");
        if (handle) CloseHandle(handle);
        if (!running.empty()) break;
    }
    CloseHandle(snapshot);
    return running;
}
DWORD run(HWND parent, const std::wstring& executable, std::wstring command) {
    STARTUPINFOW startup{}; startup.cb = sizeof(startup); startup.dwFlags = STARTF_USESHOWWINDOW; startup.wShowWindow = SW_HIDE;
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process)) return GetLastError();
    CloseHandle(process.hThread);
    for (;;) {
        const DWORD result = MsgWaitForMultipleObjects(1, &process.hProcess, FALSE, INFINITE, QS_ALLINPUT);
        if (result == WAIT_OBJECT_0) break;
        if (result != WAIT_OBJECT_0 + 1) { CloseHandle(process.hProcess); return ERROR_GEN_FAILURE; }
        MSG message{};
        while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
            // Cancellation is handled at the NSIS section boundary; interrupting
            // an MSI halfway through its transaction would damage migration.
            if (message.message == WM_QUIT) continue;
            if (!parent || !IsDialogMessageW(parent, &message)) { TranslateMessage(&message); DispatchMessageW(&message); }
        }
    }
    DWORD code = ERROR_GEN_FAILURE; GetExitCodeProcess(process.hProcess, &code); CloseHandle(process.hProcess); return code;
}
DWORD erase(const Record& r) {
    Key root(r.root, uninstallRoot, KEY_WRITE | r.view);
    if (!root.value) return ERROR_ACCESS_DENIED;
    const auto result = RegDeleteTreeW(root.value, r.key.substr(wcslen(uninstallRoot) + 1).c_str());
    return result == ERROR_FILE_NOT_FOUND ? ERROR_SUCCESS : result;
}
void write(HKEY hive, const std::wstring& path, const wchar_t* name, const std::wstring& value, std::wstring& error) {
    if (!error.empty()) return;
    HKEY key = nullptr;
    auto status = RegCreateKeyExW(hive, path.c_str(), 0, nullptr, 0, KEY_SET_VALUE | KEY_WOW64_64KEY, nullptr, &key, nullptr);
    if (status == ERROR_SUCCESS) {
        status = RegSetValueExW(key, name, 0, REG_SZ, reinterpret_cast<const BYTE*>(value.c_str()), static_cast<DWORD>((value.size() + 1) * sizeof(wchar_t)));
        RegCloseKey(key);
    }
    if (status != ERROR_SUCCESS) error = std::wstring(novaText(L"文件关联登记失败：", L"Could not register file integration: ")) +
        (hive == HKEY_LOCAL_MACHINE ? L"HKLM\\" : L"HKCU\\") + path + L" [" + (*name ? name : L"@") + L"] (" + std::to_wstring(status) + L")";
}
bool novaCommand(const std::wstring& command, const std::vector<Record>& old) {
    // CommandLineToArgvW(L"") returns the current program (our installer).
    if (command.find_first_not_of(L" \t\r\n") == std::wstring::npos) return false;
    const auto file = firstArgument(command);
    if (knownName(versionName(file))) return true;
    for (const auto& entry : old) if ((!entry.binary.empty() && sameInstallPath(file, entry.binary)) ||
            (!entry.directory.empty() && sameInstallPath(file, entry.directory + L"\\Litematica-Nova.exe"))) return true;
    return false;
}
std::wstring shortcutError(const std::wstring& link, const wchar_t* operation, HRESULT hr) {
    wchar_t code[12]{};
    swprintf_s(code, L"0x%08lX", static_cast<unsigned long>(hr));
    return std::wstring(novaText(L"旧快捷方式未更新：", L"Previous shortcut not updated: ")) + link + L" (" + operation + L", " + code + L")";
}
std::wstring repairShortcut(const std::wstring& link, const std::wstring& target, const std::vector<Record>& old) {
    if (!exists(link)) return {};
    IShellLinkW* shell = nullptr;
    HRESULT hr = CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&shell));
    if (FAILED(hr)) return shortcutError(link, L"CoCreateInstance", hr);
    IPersistFile* file = nullptr;
    const wchar_t* operation = L"QueryInterface";
    hr = shell->QueryInterface(IID_PPV_ARGS(&file));
    if (SUCCEEDED(hr)) {
        operation = L"Load";
        hr = file->Load(link.c_str(), STGM_READ | STGM_SHARE_DENY_NONE);
        if (SUCCEEDED(hr)) {
            wchar_t path[32768]{};
            const auto directory = shellPath(target);
            const auto executable = directory + L"\\Litematica-Nova.exe";
            operation = L"GetPath";
            hr = shell->GetPath(path, 32768, nullptr, SLGP_RAWPATH);
            // NSIS already created the canonical link. Do not resave it, or
            // overwrite arguments, a custom icon or a user working directory.
            if (hr == S_OK && *path && !sameInstallPath(path, executable) && novaCommand(L"\"" + std::wstring(path) + L"\"", old)) {
                operation = L"SetPath"; hr = shell->SetPath(executable.c_str());
                if (SUCCEEDED(hr)) { operation = L"SetWorkingDirectory"; hr = shell->SetWorkingDirectory(directory.c_str()); }
                if (SUCCEEDED(hr)) { operation = L"SetIconLocation"; hr = shell->SetIconLocation(executable.c_str(), 0); }
                if (SUCCEEDED(hr)) { operation = L"Save"; hr = file->Save(nullptr, TRUE); }
            }
        }
        file->Release();
    }
    shell->Release();
    return FAILED(hr) ? shortcutError(link, operation, hr) : std::wstring{};
}
}

NSIS_ENTRY(FindInstallLocation) {
    (void)parent;
    Stack stack{stringSize, stackTop};
    // The supported scope is permanently per-machine. Keep a machine install's
    // location, but migrate a historical per-user package to Program Files.
    for (const auto& record : records()) if (record.root == HKEY_LOCAL_MACHINE && !record.directory.empty() && (!record.binary.empty() || record.stable)) { stack.push(record.directory); return; }
    stack.push(L"");
}
NSIS_ENTRY(DirectoryDisplayPath) {
    (void)parent;
    Stack stack{stringSize, stackTop};
    stack.push(shellPath(stack.pop()));
}

namespace {
// Keep all ancestors open while resolving the next component. No delete sharing:
// the checked directory cannot be renamed out from underneath the elevated installer.
struct PinnedFiles {
    std::vector<HANDLE> handles;
    ~PinnedFiles() { for (auto it = handles.rbegin(); it != handles.rend(); ++it) CloseHandle(*it); }
    HANDLE open(const std::wstring& path, bool directory) {
        const DWORD access = directory ? FILE_READ_ATTRIBUTES : GENERIC_READ | DELETE;
        const DWORD flags = FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | (directory ? 0 : FILE_FLAG_SEQUENTIAL_SCAN);
        HANDLE file = CreateFileW(path.c_str(), access, FILE_SHARE_READ,
                                  nullptr, OPEN_EXISTING, flags, nullptr);
        if (file != INVALID_HANDLE_VALUE) handles.push_back(file);
        return file;
    }
};
const wchar_t* pruneOpenFailure() {
    const auto error = GetLastError();
    return error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND || error == ERROR_DIRECTORY
        ? L"skipped" : L"busy";
}
struct FileDigest {
    HCRYPTPROV provider{};
    HCRYPTHASH hash{};
    ~FileDigest() { if (hash) CryptDestroyHash(hash); if (provider) CryptReleaseContext(provider, 0); }
    bool matches(HANDLE file, const std::wstring& expected, bool& match) {
        if (!CryptAcquireContextW(&provider, nullptr, nullptr, PROV_RSA_AES, CRYPT_VERIFYCONTEXT | CRYPT_SILENT) ||
            !CryptCreateHash(provider, CALG_SHA_256, 0, 0, &hash)) return false;
        std::array<BYTE, 64 * 1024> buffer;
        DWORD read = 0;
        do {
            if (!ReadFile(file, buffer.data(), DWORD(buffer.size()), &read, nullptr) ||
                (read && !CryptHashData(hash, buffer.data(), read, 0))) return false;
        } while (read);
        std::array<BYTE, 32> bytes;
        DWORD size = DWORD(bytes.size());
        if (!CryptGetHashParam(hash, HP_HASHVAL, bytes.data(), &size, 0) || size != bytes.size()) return false;
        constexpr wchar_t digits[] = L"0123456789abcdef";
        match = true;
        for (size_t i = 0; i < bytes.size(); ++i)
            match &= digits[bytes[i] >> 4] == expected[i*2] && digits[bytes[i] & 15] == expected[i*2+1];
        return true;
    }
};
const wchar_t* pruneRetiredFile(const std::wstring& install, const std::wstring& relative,
                               const std::wstring& sizeText, const std::wstring& digest) {
    const auto root = ordinaryPath(install);
    if (!driveRootAt(root, 0) || root.size() <= 3 || relative.empty() || relative.front() == L'\\' ||
        relative.back() == L'\\' || sizeText.empty() || digest.size() != 64 ||
        digest.find_first_not_of(L"0123456789abcdef") != digest.npos) return L"skipped";
    ULONGLONG expected = 0;
    for (auto c : sizeText) {
        if (c < L'0' || c > L'9' || expected > (MAXULONGLONG - (c-L'0'))/10) return L"skipped";
        expected = expected * 10 + (c-L'0');
    }
    std::wstring path = L"\\\\?\\" + root.substr(0, 3);
    auto suffix = root.substr(3);
    if (suffix.back() != L'\\') suffix += L'\\';
    suffix += relative;
    PinnedFiles pins;
    size_t pos = 0;
    for (;;) {
        HANDLE dir = pins.open(path, true);
        if (dir == INVALID_HANDLE_VALUE) return pruneOpenFailure();
        BY_HANDLE_FILE_INFORMATION info{};
        if (!GetFileInformationByHandle(dir, &info)) return L"busy";
        if ((info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)) != FILE_ATTRIBUTE_DIRECTORY)
            return L"skipped";
        const auto next = suffix.find(L'\\', pos);
        const auto part = suffix.substr(pos, next - pos);
        if (!ordinaryComponent(part)) return L"skipped";
        if (path.back() != L'\\') path += L'\\';
        path += part;
        if (next == suffix.npos) break;
        pos = next + 1;
    }
    const auto file = pins.open(path, false);
    if (file == INVALID_HANDLE_VALUE) return pruneOpenFailure();
    BY_HANDLE_FILE_INFORMATION info{};
    if (!GetFileInformationByHandle(file, &info)) return L"busy";
    if ((info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)) ||
        ((ULONGLONG(info.nFileSizeHigh) << 32) | info.nFileSizeLow) != expected) return L"skipped";
    FileDigest hash;
    bool matches = false;
    if (!hash.matches(file, digest, matches)) return L"busy";
    if (!matches) return L"skipped";
    FILE_DISPOSITION_INFO disposition{TRUE};
    // Mark the very handle we hashed; never reopen by name after checking its contents.
    return SetFileInformationByHandle(file, FileDispositionInfo, &disposition, sizeof(disposition)) ? L"removed" : L"busy";
}
}
NSIS_ENTRY(PruneRetiredFile) {
    (void)parent;
    Stack stack{stringSize, stackTop};
    const auto root = stack.pop(), path = stack.pop(), bytes = stack.pop(), sha = lower(stack.pop());
    stack.push(pruneRetiredFile(root, path, bytes, sha));
}
NSIS_ENTRY(CheckRunning) {
    Stack stack{stringSize, stackTop}; const auto directory = normalize(stack.pop());
    const auto message = processUsing(records(), directory, false);
    stack.push(message.empty() ? L"ok" : message);
    (void)parent;
}
NSIS_ENTRY(Migrate) {
    Stack stack{stringSize, stackTop}; const auto target = normalize(stack.pop());
    const auto old = records();
    migrationRecords = old;
    const bool legacy = std::any_of(old.begin(), old.end(), [](const auto& record) { return !record.stable && (!record.binary.empty() || !record.product.empty()); });
    int argumentCount = 0;
    LPWSTR* arguments = CommandLineToArgvW(GetCommandLineW(), &argumentCount);
    bool updateHandoff = false;
    for (int i = 1; arguments && i < argumentCount; ++i) {
        // Everything after /ARGS belongs to the reopened application.
        if (equal(arguments[i], L"/ARGS")) break;
        if (equal(arguments[i], L"/UPDATE")) updateHandoff = true;
    }
    if (arguments) LocalFree(arguments);
    const auto running = processUsing(old, target, legacy, updateHandoff);
    if (!running.empty()) { stack.push(running); return; }
    std::set<std::wstring> removed;
    for (const auto& record : old) {
        // Only the canonical installation at the destination is an in-place
        // overwrite. A stable record at another directory still owns real
        // files; overwriting its ARP key alone would strand a second version.
        if (record.stable && sameInstallPath(record.directory, target)) continue;
        DWORD code = ERROR_SUCCESS;
        if (!record.product.empty() && removed.insert(lower(record.product)).second && MsiQueryProductStateW(record.product.c_str()) != INSTALLSTATE_UNKNOWN) {
            wchar_t system[MAX_PATH]{}; GetSystemDirectoryW(system, MAX_PATH);
            const std::wstring msiexec = std::wstring(system) + L"\\msiexec.exe";
            code = run(parent, msiexec, L"\"" + msiexec + L"\" /x " + record.product + L" /qn /norestart REBOOT=ReallySuppress");
            if (code == ERROR_SUCCESS_REBOOT_REQUIRED) code = ERROR_SUCCESS;
        } else if (!record.binary.empty() && exists(record.uninstaller) && removed.insert(lower(record.uninstaller)).second) {
            // All old removals happen before File/WriteUninstaller, including a
            // shared $INSTDIR. /UPDATE preserves the user's app data.
            code = run(parent, record.uninstaller, L"\"" + record.uninstaller + L"\" /S /UPDATE _?=" + record.directory);
        }
        if (code != ERROR_SUCCESS) { stack.push(std::wstring(novaText(L"旧版迁移失败（", L"Could not migrate the previous installation (")) + record.name + novaText(L"，退出码 ", L", exit code ") + std::to_wstring(code) + novaText(L"），新文件尚未写入", L"). No new files have been written.")); return; }
        if (!record.binary.empty() && exists(record.binary)) { stack.push(std::wstring(novaText(L"旧版程序仍在原安装目录，迁移已停止：", L"Migration stopped because the previous application remains installed: ")) + record.binary); return; }
        const auto erased = erase(record);
        if (erased != ERROR_SUCCESS) { stack.push(std::wstring(novaText(L"旧版安装登记迁移失败，错误码 ", L"Could not migrate the previous installation registration; error code ")) + std::to_wstring(erased)); return; }
    }
    stack.push(L"ok");
}

NSIS_ENTRY(RepairShell) {
    (void)parent;
    Stack stack{stringSize, stackTop}; const auto target = normalize(stack.pop());
    auto old = records();
    old.insert(old.end(), migrationRecords.begin(), migrationRecords.end());
    std::wstring registrationError;
    const std::wstring command = L"\"" + target + L"\\Litematica-Nova.exe\" \"%1\" %*";
    // HKCU overrides HKLM. Repair only a command whose target is a verified Nova
    // payload; do not override Editor, UserChoice hashes, or other applications.
    Key extension(HKEY_CURRENT_USER, L"Software\\Classes\\.litematic", KEY_READ | KEY_WOW64_64KEY);
    const auto progid = read(extension.value, L"");
    if (!progid.empty()) {
        Key existing(HKEY_CURRENT_USER, L"Software\\Classes\\" + progid + L"\\shell\\open\\command", KEY_READ | KEY_WOW64_64KEY);
        if (novaCommand(read(existing.value, L""), old)) write(HKEY_CURRENT_USER, L"Software\\Classes\\" + progid + L"\\shell\\open\\command", L"", command, registrationError);
    }
    for (const auto& name : {L"LitematicaNova.Projection", L"Litematica projection", L"litematic_auto_file"}) {
        for (HKEY hive : {HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE}) {
            const auto path = std::wstring(L"Software\\Classes\\") + name + L"\\shell\\open\\command";
            Key existing(hive, path, KEY_READ | KEY_WOW64_64KEY);
            if (novaCommand(read(existing.value, L""), old)) write(hive, path, L"", command, registrationError);
        }
    }
    // Per-user Applications and Run entries override the repaired machine
    // registration and can otherwise reopen a removed or obsolete executable.
    // Preserve arguments and entries owned by any other application.
    for (HKEY hive : {HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE}) {
        for (const auto& binary : {L"Litematica-Nova.exe", L"Litematica Nova.exe", L"Litematica Nova BA.exe"}) {
            const auto path = std::wstring(L"Software\\Classes\\Applications\\") + binary + L"\\shell\\open\\command";
            Key existing(hive, path, KEY_READ | KEY_WOW64_64KEY);
            if (novaCommand(read(existing.value, L""), old)) write(hive, path, L"", command, registrationError);
        }
        const std::wstring runPath = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        Key runKey(hive, runPath, KEY_READ | KEY_WOW64_64KEY);
        for (const auto& name : names) {
            const auto previous = read(runKey.value, name.c_str());
            if (!novaCommand(previous, old)) continue;
            const auto executable = firstArgument(previous);
            const auto quoted = L"\"" + executable + L"\"";
            const bool quotedPrefix = previous.compare(0, quoted.size(), quoted) == 0;
            if (!quotedPrefix && previous.compare(0, executable.size(), executable) != 0) continue;
            const auto prefixSize = quotedPrefix ? quoted.size() : executable.size();
            write(hive, runPath, name.c_str(), L"\"" + target + L"\\Litematica-Nova.exe\"" + previous.substr(prefixSize), registrationError);
        }
    }
    const auto application = L"Software\\Classes\\Applications\\Litematica-Nova.exe";
    write(HKEY_LOCAL_MACHINE, application, L"FriendlyAppName", L"Litematica-Nova", registrationError);
    write(HKEY_LOCAL_MACHINE, std::wstring(application) + L"\\SupportedTypes", L".litematic", L"", registrationError);
    write(HKEY_LOCAL_MACHINE, std::wstring(application) + L"\\shell\\open\\command", L"", command, registrationError);
    write(HKEY_LOCAL_MACHINE, std::wstring(application) + L"\\shell\\open", L"MultiSelectModel", L"Player", registrationError);
    if (!registrationError.empty()) { stack.push(registrationError); return; }
    std::wstring warning;
    const auto repair = [&](const std::wstring& link) {
        auto result = repairShortcut(link, target, old);
        if (warning.empty() && !result.empty()) warning = std::move(result);
    };
    const HRESULT initialized = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    for (int folder : {CSIDL_COMMON_PROGRAMS, CSIDL_PROGRAMS, CSIDL_COMMON_DESKTOPDIRECTORY, CSIDL_DESKTOPDIRECTORY}) {
        wchar_t path[MAX_PATH]{};
        if (FAILED(SHGetFolderPathW(nullptr, folder, nullptr, SHGFP_TYPE_CURRENT, path))) continue;
        for (const auto& name : names) {
            repair(std::wstring(path) + L"\\" + name + L".lnk");
            if (folder == CSIDL_COMMON_PROGRAMS || folder == CSIDL_PROGRAMS) repair(std::wstring(path) + L"\\" + name + L"\\" + name + L".lnk");
        }
    }
    if (SUCCEEDED(initialized)) CoUninitialize();
    SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, nullptr, nullptr);
    stack.push(warning.empty() ? L"ok" : L"warning: " + warning);
}
