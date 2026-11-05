// Native x86 CLI regression. Supply a fresh empty scratch directory; no shell
// folders, UI, HKLM or existing user links are touched.
#include "../migration.cpp"
#include <chrono>
#include <filesystem>
#include <cstdio>
#include <stdexcept>

namespace fs = std::filesystem;
void require(bool ok, const char* what) { if (!ok) throw std::runtime_error(what); }
void checkHr(HRESULT hr, const char* what) {
    if (FAILED(hr)) { std::fprintf(stderr, "%s: 0x%08lX\n", what, static_cast<unsigned long>(hr)); throw std::runtime_error(what); }
}
struct Link {
    IShellLinkW* shell{}; IPersistFile* file{};
    Link() {
        checkHr(CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&shell)), "create shell link");
        checkHr(shell->QueryInterface(IID_PPV_ARGS(&file)), "query persist file");
    }
    ~Link() { file->Release(); shell->Release(); }
};
void make(const fs::path& file, const fs::path& exe) {
    Link l;
    checkHr(l.shell->SetPath(exe.c_str()), "set target");
    checkHr(l.shell->SetArguments(L"--keep-me"), "set arguments");
    checkHr(l.shell->SetWorkingDirectory(exe.parent_path().c_str()), "set workdir");
    checkHr(l.file->Save(file.c_str(), TRUE), "create fixture");
}
void checkLink(const fs::path& file, const fs::path& exe) {
    Link l;
    checkHr(l.file->Load(file.c_str(), STGM_READ | STGM_SHARE_DENY_NONE), "read fixture");
    wchar_t result[32768]{};
    checkHr(l.shell->GetPath(result, 32768, nullptr, SLGP_RAWPATH), "get target");
    require(equal(result, exe.wstring()), "wrong target");
    checkHr(l.shell->GetWorkingDirectory(result, 32768), "get working directory");
    require(equal(result, exe.parent_path().wstring()), "wrong shell working directory");
    checkHr(l.shell->GetArguments(result, 32768), "get arguments");
    require(std::wstring(result) == L"--keep-me", "lost arguments");
}
int wmain(int argc, wchar_t** argv) {
    try {
        require(argc == 2, "supply a fresh empty scratch directory");
        const auto dir = fs::absolute(argv[1]);
        require(fs::is_directory(dir) && fs::is_empty(dir), "scratch directory is not empty");
        checkHr(CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED), "initialize COM");
        const auto previous = dir / L"previous", dest = dir / L"current";
        fs::create_directory(previous); fs::create_directory(dest);
        const auto from = previous / L"Litematica-Nova.exe", to = dest / L"Litematica-Nova.exe", foreign = dir / L"other.exe";
        for (const auto& p : {from, to, foreign}) {
            const auto h = CreateFileW(p.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
            require(h != INVALID_HANDLE_VALUE, "create fixture target"); CloseHandle(h);
        }
        Record legacy{}; legacy.directory = previous; legacy.binary = from; const std::vector<Record> old{legacy};
        const auto same = dir / L"canonical.lnk", unrelated = dir / L"other.lnk", stale = dir / L"old.lnk";
        make(same, to); make(unrelated, foreign); make(stale, from);
        for (const auto& p : {same, unrelated}) {
            auto stamp = fs::last_write_time(p); stamp -= std::chrono::hours(24); fs::last_write_time(p, stamp);
            stamp = fs::last_write_time(p);
            require(repairShortcut(p, dest, old).empty(), "current or foreign link repair failed");
            require(fs::last_write_time(p) == stamp, "current or foreign link rewritten");
        }
        checkLink(same, to); checkLink(unrelated, foreign);
        const auto locked = CreateFileW(stale.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
        require(locked != INVALID_HANDLE_VALUE, "hold stale link read-only");
        const auto issue = repairShortcut(stale, dest, old);
        CloseHandle(locked);
        require(issue.find(stale.wstring()) != std::wstring::npos && issue.find(L"Save, 0x") != std::wstring::npos, "missing actual shortcut Save error");
        std::wprintf(L"LOCKED_SHORTCUT %ls\n", issue.c_str());
        checkLink(stale, from);
        const auto path = dest.wstring();
        const auto verbatim = path.compare(0, 2, L"\\\\") == 0
            ? L"\\\\?\\UNC\\" + path.substr(2) : L"\\\\?\\" + path;
        require(repairShortcut(stale, verbatim, old).empty(), "legacy link not repaired for verbatim destination");
        checkLink(stale, to);
        require(!novaCommand(L"", old) && !novaCommand(L" \t ", old), "empty registry command identifies installer");
        CoUninitialize();
        std::wprintf(L"NOVA_SHORTCUT_REPAIR_OK canonical-unchanged foreign-unchanged locked-diagnostic legacy-migrated args-preserved empty-command-ignored\n");
        return 0;
    } catch (const std::exception& e) { std::fprintf(stderr, "FAIL %s\n", e.what()); return 1; }
}
