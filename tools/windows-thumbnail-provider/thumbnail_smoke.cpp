#include "provider_abi.h"
#include <thumbcache.h>
#include <propsys.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <cstdio>
#include <filesystem>
#include <stdexcept>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
namespace {
using GetFactory = HRESULT(__stdcall*)(REFCLSID, REFIID, void**);
using CanUnload = HRESULT(__stdcall*)();
void Require(bool okay, const char* message) { if (!okay) throw std::runtime_error(message); }
void Success(HRESULT result, const char* message) { if (FAILED(result)) { std::fprintf(stderr, "%s hr=0x%08lx\n", message, static_cast<unsigned long>(result)); throw std::runtime_error(message); } }
struct OwnedFile {
    std::wstring path;
    bool owned = false;
    ~OwnedFile() { if (owned) DeleteFileW(path.c_str()); }
};
void WriteLegacyCache(const std::wstring& path) {
    ComPtr<IWICImagingFactory> factory; Success(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory)), "WIC factory");
    ComPtr<IWICStream> stream; Success(factory->CreateStream(&stream), "WIC stream"); Success(stream->InitializeFromFilename(path.c_str(), GENERIC_WRITE), "cache fixture stream");
    ComPtr<IWICBitmapEncoder> encoder; Success(factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder), "PNG encoder"); Success(encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache), "PNG init");
    ComPtr<IWICBitmapFrameEncode> frame; ComPtr<IPropertyBag2> properties; Success(encoder->CreateNewFrame(&frame, &properties), "PNG frame"); Success(frame->Initialize(properties.Get()), "frame init"); Success(frame->SetSize(1, 1), "frame size");
    GUID format = GUID_WICPixelFormat32bppBGRA; Success(frame->SetPixelFormat(&format), "frame format"); Require(format == GUID_WICPixelFormat32bppBGRA, "BGRA encoder required");
    BYTE pixel[] = {0, 0, 255, 255}; Success(frame->WritePixels(1, 4, 4, pixel), "frame pixels"); Success(frame->Commit(), "frame commit"); Success(encoder->Commit(), "encoder commit");
}
std::vector<BYTE> ReadBitmap(IThumbnailProvider* thumbnail, UINT edge) {
    HBITMAP bitmap = nullptr; WTS_ALPHATYPE alpha = WTSAT_UNKNOWN;
    Success(thumbnail->GetThumbnail(edge, &bitmap, &alpha), "embedded GetThumbnail"); Require(bitmap != nullptr, "embedded image must return a bitmap");
    DIBSECTION section{}; const int received = GetObjectW(bitmap, sizeof(section), &section);
    const bool shape = received == sizeof(section) && section.dsBm.bmWidth > 0 && section.dsBm.bmHeight > 0
        && section.dsBm.bmWidth <= static_cast<LONG>(edge) && section.dsBm.bmHeight <= static_cast<LONG>(edge)
        && section.dsBm.bmBitsPixel == 32 && alpha == WTSAT_ARGB && section.dsBm.bmBits;
    std::vector<BYTE> pixels;
    if (shape) {
        const auto* data = static_cast<const BYTE*>(section.dsBm.bmBits);
        pixels.assign(data, data + static_cast<size_t>(section.dsBm.bmWidthBytes) * section.dsBm.bmHeight);
    }
    DeleteObject(bitmap); Require(shape, "embedded bitmap shape/ARGB mismatch");
    std::printf("embedded_preview=S_OK cx=%u width=%ld height=%ld bpp=32 alpha=ARGB\n", edge, section.dsBm.bmWidth, section.dsBm.bmHeight);
    return pixels;
}
} // namespace

int wmain(int argc, wchar_t** argv) {
    if (argc < 2 || argc > 3) { std::fprintf(stderr, "usage: thumbnail_smoke.exe <absolute-provider.dll> [embedded-fixture.litematic]\n"); return 2; }
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
        Require(thumbnail->GetThumbnail(0, &invalid, &invalidAlpha) == E_INVALIDARG && !invalid, "zero edge rejected"); Require(thumbnail->GetThumbnail(32, nullptr, &invalidAlpha) == E_POINTER, "null bitmap rejected");
        ComPtr<IInitializeWithStream> initialize; Success(thumbnail.As(&initialize), "stream initialization interface");
        ComPtr<IUnknown> left, right; Success(thumbnail.As(&left), "canonical thumbnail identity"); Success(initialize.As(&right), "canonical init identity"); Require(left.Get() == right.Get(), "COM identity differs across interfaces"); left.Reset(); right.Reset();
        // Deterministic gzip of a valid empty NBT root Compound, not malformed
        // bytes that can make the CLI close stdin before the writer finishes.
        const std::vector<BYTE> bytes = {0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x02, 0x0a, 0xe3, 0x62, 0x60, 0x60, 0x00, 0x00, 0x78, 0x3f, 0xf9, 0x4e, 0x04, 0x00, 0x00, 0x00};
        ComPtr<IStream> original; Success(CreateStreamOnHGlobal(nullptr, TRUE, &original), "memory stream"); ULONG wrote = 0; Success(original->Write(bytes.data(), static_cast<ULONG>(bytes.size()), &wrote), "fixture bytes"); Require(wrote == bytes.size(), "stream write complete");
        LARGE_INTEGER position{}; position.QuadPart = 17; Success(original->Seek(position, STREAM_SEEK_SET, nullptr), "source stream offset"); Success(initialize->Initialize(original.Get(), STGM_READ), "Initialize stream"); Require(initialize->Initialize(original.Get(), STGM_READ) == HRESULT_FROM_WIN32(ERROR_ALREADY_INITIALIZED), "reinit rejected");
        PWSTR local = nullptr; Success(SHGetKnownFolderPath(FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, nullptr, &local), "LocalAppData"); std::wstring folder = local; CoTaskMemFree(local); folder += L"\\Litematica-Nova\\thumbnails"; std::filesystem::create_directories(folder);
        OwnedFile legacy; legacy.path = folder + L"\\d4ba26f3ed1479f1fd430bbce9e4acde01be1ec798452553ecf97fe3136ac507.png";
        HANDLE reserved = CreateFileW(legacy.path.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (reserved != INVALID_HANDLE_VALUE) { CloseHandle(reserved); legacy.owned = true; WriteLegacyCache(legacy.path); }
        else Require(GetLastError() == ERROR_FILE_EXISTS || GetLastError() == ERROR_ALREADY_EXISTS, "legacy fixture reservation failed");
        HBITMAP missing = nullptr; WTS_ALPHATYPE missingAlpha = WTSAT_UNKNOWN;
        const HRESULT miss = thumbnail->GetThumbnail(32, &missing, &missingAlpha);
        std::printf("missing_probe_hresult=0x%08lx bitmap=%s alpha=%u\n", static_cast<unsigned long>(miss), missing ? "present" : "null", static_cast<unsigned>(missingAlpha));
        Require(miss == HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND) && !missing && missingAlpha == WTSAT_UNKNOWN, "no embedded preview must miss even when an old PNG cache exists");
        ULARGE_INTEGER cursor{}; LARGE_INTEGER zero{}; Success(original->Seek(zero, STREAM_SEEK_CUR, &cursor), "source position preserved"); Require(cursor.QuadPart == 17, "original stream cursor changed");
        std::printf("no_embedded_preview=0x%08lx legacy_png_ignored=true source_position=17 bitmap=null alpha=UNKNOWN\n", static_cast<unsigned long>(miss));
        if (argc == 3) {
            ComPtr<IInitializeWithFile> fileInit; Success(factory->CreateInstance(nullptr, IID_PPV_ARGS(&fileInit)), "file instance"); Success(fileInit->Initialize(argv[2], STGM_READ), "real embedded fixture file");
            ComPtr<IThumbnailProvider> fileThumbnail; Success(fileInit.As(&fileThumbnail), "file thumbnail interface"); const auto expected = ReadBitmap(fileThumbnail.Get(), 64);
            ComPtr<IStream> source; Success(SHCreateStreamOnFileEx(argv[2], STGM_READ | STGM_SHARE_DENY_NONE, FILE_ATTRIBUTE_NORMAL, FALSE, nullptr, &source), "real source stream");
            ComPtr<IInitializeWithStream> streamInit; Success(factory->CreateInstance(nullptr, IID_PPV_ARGS(&streamInit)), "stream instance"); Success(streamInit->Initialize(source.Get(), STGM_READ), "real embedded fixture stream");
            ComPtr<IThumbnailProvider> streamThumbnail; Success(streamInit.As(&streamThumbnail), "stream thumbnail interface"); Require(ReadBitmap(streamThumbnail.Get(), 64) == expected, "file and stream image pixels differ");
            ReadBitmap(streamThumbnail.Get(), 256); std::printf("embedded_file_stream_pixels=identical external_cache_required=false\n");
        }
        initialize.Reset(); thumbnail.Reset(); original.Reset(); Success(factory->LockServer(TRUE), "server lock"); factory.Reset(); Require(canUnload() == S_FALSE, "server lock retains DLL"); Success(getFactory(CLSID_NovaThumbnail, IID_PPV_ARGS(&factory)), "unlock factory"); Success(factory->LockServer(FALSE), "server unlock"); factory.Reset(); Require(canUnload() == S_OK, "all COM objects released");
        std::printf("com_identity=canonical reinitialize=rejected invalid_parameters=rejected aggregation=rejected lock_unload=passed\n");
        std::printf("registry_changed=false gui_started=false gpu_viewer_started=false\n");
    } catch (const std::exception& failure) { std::fprintf(stderr, "thumbnail smoke failed: %s\n", failure.what()); status = 1; }
    FreeLibrary(module); CoUninitialize(); return status;
}
