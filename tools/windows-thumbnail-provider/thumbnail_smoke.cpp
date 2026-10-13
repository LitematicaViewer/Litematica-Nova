#include "provider_abi.h"
#include <thumbcache.h>
#include <propsys.h>
#include <shlobj.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <cstdio>
#include <filesystem>
#include <stdexcept>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
namespace {
constexpr wchar_t fixtureDigest[] = L"e11d9da40d26f5839c8b15aedb0aa0fa8753b52bf84732b8af39d40123ea4316";
using GetFactory = HRESULT(__stdcall*)(REFCLSID, REFIID, void**);
using CanUnload = HRESULT(__stdcall*)();
void Require(bool okay, const char* message) { if (!okay) throw std::runtime_error(message); }
void Success(HRESULT result, const char* message) { if (FAILED(result)) { std::fprintf(stderr, "%s hr=0x%08lx\n", message, static_cast<unsigned long>(result)); throw std::runtime_error(message); } }
struct OwnedFiles {
    std::wstring png;
    std::wstring source;
    bool pngOwned = false;
    bool sourceOwned = false;
    ~OwnedFiles() { if (pngOwned) DeleteFileW(png.c_str()); if (sourceOwned) DeleteFileW(source.c_str()); }
};
void ReserveFile(const std::wstring& path) {
    HANDLE file = CreateFileW(path.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    Require(file != INVALID_HANDLE_VALUE, "unit fixture path exists or cannot be created; no overwrite attempted"); CloseHandle(file);
}
void WriteFixturePng(const std::wstring& path) {
    ComPtr<IWICImagingFactory> factory; Success(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory)), "WIC factory");
    ComPtr<IWICStream> stream; Success(factory->CreateStream(&stream), "WIC stream"); Success(stream->InitializeFromFilename(path.c_str(), GENERIC_WRITE), "fixture stream");
    ComPtr<IWICBitmapEncoder> encoder; Success(factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder), "PNG encoder"); Success(encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache), "PNG init");
    ComPtr<IWICBitmapFrameEncode> frame; ComPtr<IPropertyBag2> properties; Success(encoder->CreateNewFrame(&frame, &properties), "PNG frame"); Success(frame->Initialize(properties.Get()), "frame init"); Success(frame->SetSize(64, 32), "frame size");
    GUID format = GUID_WICPixelFormat32bppBGRA; Success(frame->SetPixelFormat(&format), "frame format"); Require(format == GUID_WICPixelFormat32bppBGRA, "expected BGRA encoder input");
    std::vector<BYTE> pixels(64 * 32 * 4); for (size_t i = 0; i < pixels.size(); i += 4) { pixels[i + 2] = 255; pixels[i + 3] = 128; }
    Success(frame->WritePixels(32, 64 * 4, static_cast<UINT>(pixels.size()), pixels.data()), "frame pixels"); Success(frame->Commit(), "frame commit"); Success(encoder->Commit(), "encoder commit");
}
void CheckBitmap(IThumbnailProvider* thumbnail, UINT cx, LONG width, LONG height) {
    HBITMAP bitmap = nullptr; WTS_ALPHATYPE alpha = WTSAT_UNKNOWN;
    Success(thumbnail->GetThumbnail(cx, &bitmap, &alpha), "cache-hit GetThumbnail"); Require(bitmap != nullptr, "bitmap is required on cache hit");
    DIBSECTION section{}; const int received = GetObjectW(bitmap, sizeof(section), &section);
    const bool shape = received == sizeof(section) && section.dsBm.bmWidth == width && section.dsBm.bmHeight == height && section.dsBm.bmBitsPixel == 32;
    const auto* pixels = static_cast<const BYTE*>(section.dsBm.bmBits);
    const bool validAlpha = pixels && alpha == WTSAT_ARGB && pixels[3] == 128 && pixels[2] >= 127 && pixels[2] <= 129;
    DeleteObject(bitmap); Require(shape, "DIB shape/aspect/32bpp mismatch"); Require(validAlpha, "premultiplied alpha mismatch");
    std::printf("png_cache_hit=S_OK cx=%u size=%ldx%ld bpp=32 alpha=ARGB premultiplied_red=128\n", cx, width, height);
}
} // namespace

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) { std::fprintf(stderr, "usage: thumbnail_smoke.exe <absolute-provider.dll>\n"); return 2; }
    const HRESULT initialized = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(initialized)) return 3;
    HMODULE module = LoadLibraryExW(argv[1], nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) { CoUninitialize(); return 4; }
    int status = 0;
    try {
        const auto getFactory = reinterpret_cast<GetFactory>(GetProcAddress(module, "DllGetClassObject"));
        const auto canUnload = reinterpret_cast<CanUnload>(GetProcAddress(module, "DllCanUnloadNow"));
        Require(getFactory && canUnload, "COM exports missing"); Require(GetProcAddress(module, "DllRegisterServer") == nullptr, "self-registration must not be exported"); Require(canUnload() == S_OK, "new DLL must be unloadable");
        ComPtr<IClassFactory> factory; Success(getFactory(CLSID_NovaThumbnail, IID_PPV_ARGS(&factory)), "class factory"); Require(canUnload() == S_FALSE, "factory holds DLL");
        void* rejected = nullptr; Require(getFactory(CLSID_NULL, IID_IUnknown, &rejected) == CLASS_E_CLASSNOTAVAILABLE && !rejected, "unknown CLSID rejected");
        Require(factory->CreateInstance(factory.Get(), IID_IThumbnailProvider, &rejected) == CLASS_E_NOAGGREGATION && !rejected, "aggregation rejected");
        ComPtr<IThumbnailProvider> thumbnail; Success(factory->CreateInstance(nullptr, IID_PPV_ARGS(&thumbnail)), "thumbnail instance");
        HBITMAP invalid = nullptr; WTS_ALPHATYPE invalidAlpha = WTSAT_UNKNOWN;
        Require(thumbnail->GetThumbnail(32, &invalid, &invalidAlpha) == E_UNEXPECTED && !invalid, "uninitialized provider rejected");
        Require(thumbnail->GetThumbnail(0, &invalid, &invalidAlpha) == E_INVALIDARG && !invalid, "zero cx rejected"); Require(thumbnail->GetThumbnail(32, nullptr, &invalidAlpha) == E_POINTER, "null bitmap rejected");
        ComPtr<IInitializeWithStream> initialize; Success(thumbnail.As(&initialize), "stream initialization interface");
        ComPtr<IUnknown> left, right; Success(thumbnail.As(&left), "canonical thumbnail identity"); Success(initialize.As(&right), "canonical init identity"); Require(left.Get() == right.Get(), "COM identity differs across interfaces"); left.Reset(); right.Reset();
        std::vector<BYTE> bytes(131077); for (size_t i = 0; i < bytes.size(); ++i) bytes[i] = static_cast<BYTE>((i * 13) % 251);
        ComPtr<IStream> original; Success(CreateStreamOnHGlobal(nullptr, TRUE, &original), "memory stream"); ULONG wrote = 0; Success(original->Write(bytes.data(), static_cast<ULONG>(bytes.size()), &wrote), "fixture bytes"); Require(wrote == bytes.size(), "stream write complete");
        LARGE_INTEGER position{}; position.QuadPart = 17; Success(original->Seek(position, STREAM_SEEK_SET, nullptr), "source stream offset"); Success(initialize->Initialize(original.Get(), STGM_READ), "Initialize stream"); Require(initialize->Initialize(original.Get(), STGM_READ) == HRESULT_FROM_WIN32(ERROR_ALREADY_INITIALIZED), "reinit rejected");
        OwnedFiles owned; PWSTR local = nullptr; Success(SHGetKnownFolderPath(FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, nullptr, &local), "LocalAppData"); std::wstring folder = local; CoTaskMemFree(local); folder += L"\\Litematica-Nova\\thumbnails"; std::filesystem::create_directories(folder); owned.png = folder + L"\\" + fixtureDigest + L".png";
        Require(GetFileAttributesW(owned.png.c_str()) == INVALID_FILE_ATTRIBUTES && GetLastError() == ERROR_FILE_NOT_FOUND, "fixture cache miss must be genuinely absent");
        HBITMAP missing = nullptr; WTS_ALPHATYPE missingAlpha = WTSAT_UNKNOWN;
        const HRESULT miss = thumbnail->GetThumbnail(32, &missing, &missingAlpha); Require(miss == HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND) && !missing && missingAlpha == WTSAT_UNKNOWN, "normal missing HRESULT without fabricated bitmap");
        std::printf("stream_cache_miss=0x%08lx bitmap=null alpha=UNKNOWN\n", static_cast<unsigned long>(miss));
        ReserveFile(owned.png); owned.pngOwned = true; WriteFixturePng(owned.png); CheckBitmap(thumbnail.Get(), 32, 32, 16); CheckBitmap(thumbnail.Get(), 1024, 64, 32);
        HANDLE broken = CreateFileW(owned.png.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr); Require(broken != INVALID_HANDLE_VALUE, "owned PNG corruption fixture open"); DWORD brokenCount = 0; const char garbage[] = "not a PNG"; const BOOL brokenWritten = WriteFile(broken, garbage, sizeof(garbage), &brokenCount, nullptr); const BOOL truncated = SetEndOfFile(broken); CloseHandle(broken); Require(brokenWritten && truncated, "owned PNG corruption fixture write");
        HBITMAP corruptBitmap = nullptr; WTS_ALPHATYPE corruptAlpha = WTSAT_UNKNOWN; const HRESULT corrupt = thumbnail->GetThumbnail(32, &corruptBitmap, &corruptAlpha); Require(FAILED(corrupt) && !corruptBitmap && corruptAlpha == WTSAT_UNKNOWN, "corrupt cache must not produce a fabricated bitmap"); std::printf("corrupt_png_cache=0x%08lx bitmap=null alpha=UNKNOWN\n", static_cast<unsigned long>(corrupt)); WriteFixturePng(owned.png);
        ULARGE_INTEGER cursor{}; LARGE_INTEGER zero{}; Success(original->Seek(zero, STREAM_SEEK_CUR, &cursor), "source position preserved"); Require(cursor.QuadPart == 17, "original stream cursor changed");
        std::printf("stream_hash=e11d9da40d26f5839c8b15aedb0aa0fa8753b52bf84732b8af39d40123ea4316 bytes=131077 source_position=17\n");
        owned.source = (std::filesystem::path(argv[1]).parent_path() / L"thumbnail-unit-fixture.litematic").wstring(); ReserveFile(owned.source); owned.sourceOwned = true;
        HANDLE file = CreateFileW(owned.source.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr); Require(file != INVALID_HANDLE_VALUE, "fixture source open"); DWORD count = 0; const BOOL fileWritten = WriteFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &count, nullptr); CloseHandle(file); Require(fileWritten && count == bytes.size(), "fixture source write complete");
        ComPtr<IInitializeWithFile> fileInit; Success(factory->CreateInstance(nullptr, IID_PPV_ARGS(&fileInit)), "file instance"); Success(fileInit->Initialize(owned.source.c_str(), STGM_READ), "file initializer"); ComPtr<IThumbnailProvider> fileThumbnail; Success(fileInit.As(&fileThumbnail), "file thumbnail interface"); CheckBitmap(fileThumbnail.Get(), 32, 32, 16); fileThumbnail.Reset(); fileInit.Reset();
        initialize.Reset(); thumbnail.Reset(); original.Reset(); Success(factory->LockServer(TRUE), "server lock"); factory.Reset(); Require(canUnload() == S_FALSE, "server lock retains DLL"); Success(getFactory(CLSID_NovaThumbnail, IID_PPV_ARGS(&factory)), "unlock factory"); Success(factory->LockServer(FALSE), "server unlock"); factory.Reset(); Require(canUnload() == S_OK, "all COM objects released");
        std::printf("com_identity=canonical reinitialize=rejected invalid_parameters=rejected aggregation=rejected lock_unload=passed\n");
        std::printf("fixture_only=true registry_changed=false gui_started=false renderer_or_parser_loaded=false\n");
    } catch (const std::exception& failure) { std::fprintf(stderr, "thumbnail smoke failed: %s\n", failure.what()); status = 1; }
    FreeLibrary(module); CoUninitialize(); return status;
}
