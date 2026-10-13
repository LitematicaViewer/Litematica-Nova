#include "provider_abi.h"
#include <thumbcache.h>
#include <propsys.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <wincodec.h>
#include <bcrypt.h>
#include <wrl/client.h>
#include <array>
#include <climits>
#include <atomic>
#include <algorithm>
#include <new>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
namespace {
std::atomic<long> activeObjects{0};
std::atomic<long> serverLocks{0};

HRESULT NtResult(NTSTATUS status) noexcept { return status < 0 ? HRESULT_FROM_NT(status) : S_OK; }
struct Algorithm {
    BCRYPT_ALG_HANDLE value = nullptr;
    ~Algorithm() { if (value) BCryptCloseAlgorithmProvider(value, 0); }
};
struct Hash {
    BCRYPT_HASH_HANDLE value = nullptr;
    ~Hash() { if (value) BCryptDestroyHash(value); }
};
struct StreamPosition {
    IStream* stream;
    ULARGE_INTEGER original{};
    bool known = false;
    explicit StreamPosition(IStream* input) : stream(input) { LARGE_INTEGER zero{}; known = SUCCEEDED(stream->Seek(zero, STREAM_SEEK_CUR, &original)); }
    ~StreamPosition() { if (known && original.QuadPart <= static_cast<ULONGLONG>(LLONG_MAX)) { LARGE_INTEGER position{}; position.QuadPart = static_cast<LONGLONG>(original.QuadPart); stream->Seek(position, STREAM_SEEK_SET, nullptr); } }
};

HRESULT HashOriginalStream(IStream* stream, std::wstring& hex) {
    StreamPosition restore(stream);
    LARGE_INTEGER zero{};
    HRESULT hr = stream->Seek(zero, STREAM_SEEK_SET, nullptr);
    if (FAILED(hr)) return hr;
    Algorithm algorithm;
    hr = NtResult(BCryptOpenAlgorithmProvider(&algorithm.value, BCRYPT_SHA256_ALGORITHM, nullptr, 0));
    if (FAILED(hr)) return hr;
    DWORD objectSize = 0, received = 0;
    hr = NtResult(BCryptGetProperty(algorithm.value, BCRYPT_OBJECT_LENGTH, reinterpret_cast<PUCHAR>(&objectSize), sizeof(objectSize), &received, 0));
    if (FAILED(hr)) return hr;
    if (!objectSize || objectSize > 1024 * 1024) return E_UNEXPECTED;
    std::vector<UCHAR> object(objectSize);
    Hash hash;
    hr = NtResult(BCryptCreateHash(algorithm.value, &hash.value, object.data(), objectSize, nullptr, 0, 0));
    if (FAILED(hr)) return hr;
    std::array<UCHAR, 64 * 1024> buffer{};
    while (true) {
        ULONG count = 0;
        hr = stream->Read(buffer.data(), static_cast<ULONG>(buffer.size()), &count);
        if (FAILED(hr)) return hr;
        if (count > buffer.size()) return E_UNEXPECTED;
        if (count) { const HRESULT update = NtResult(BCryptHashData(hash.value, buffer.data(), count, 0)); if (FAILED(update)) return update; }
        if (!count || hr == S_FALSE) break;
    }
    std::array<UCHAR, 32> digest{};
    hr = NtResult(BCryptFinishHash(hash.value, digest.data(), static_cast<ULONG>(digest.size()), 0));
    if (FAILED(hr)) return hr;
    static constexpr wchar_t digits[] = L"0123456789abcdef";
    hex.clear(); hex.reserve(64);
    for (UCHAR byte : digest) { hex.push_back(digits[byte >> 4]); hex.push_back(digits[byte & 15]); }
    return S_OK;
}

HRESULT CachePath(const std::wstring& digest, std::wstring& path) {
    PWSTR local = nullptr;
    const HRESULT hr = SHGetKnownFolderPath(FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, nullptr, &local);
    if (FAILED(hr)) return hr;
    try { path = local; path += L"\\Litematica-Nova\\thumbnails\\"; path += digest; path += L".png"; }
    catch (...) { CoTaskMemFree(local); throw; }
    CoTaskMemFree(local);
    return S_OK;
}

HRESULT CachedPngBitmap(const std::wstring& path, UINT cx, HBITMAP* output) {
    const DWORD attributes = GetFileAttributesW(path.c_str());
    if (attributes == INVALID_FILE_ATTRIBUTES) { const DWORD error = GetLastError(); return HRESULT_FROM_WIN32(error == ERROR_PATH_NOT_FOUND ? ERROR_FILE_NOT_FOUND : error); }
    if (attributes & FILE_ATTRIBUTE_DIRECTORY) return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
    ComPtr<IWICImagingFactory> factory;
    HRESULT hr = CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
    if (FAILED(hr)) return hr;
    ComPtr<IWICBitmapDecoder> decoder;
    hr = factory->CreateDecoderFromFilename(path.c_str(), nullptr, GENERIC_READ, WICDecodeMetadataCacheOnDemand, &decoder);
    if (FAILED(hr)) return hr;
    GUID container{};
    hr = decoder->GetContainerFormat(&container);
    if (FAILED(hr)) return hr;
    if (container != GUID_ContainerFormatPng) return WINCODEC_ERR_BADIMAGE;
    ComPtr<IWICBitmapFrameDecode> frame;
    hr = decoder->GetFrame(0, &frame);
    if (FAILED(hr)) return hr;
    UINT width = 0, height = 0;
    hr = frame->GetSize(&width, &height);
    if (FAILED(hr)) return hr;
    if (!width || !height || width > 32768 || height > 32768 || static_cast<ULONGLONG>(width) * height > 16ULL * 1024 * 1024) return WINCODEC_ERR_IMAGESIZEOUTOFRANGE;
    UINT outWidth = width, outHeight = height;
    const UINT longest = std::max(width, height);
    if (longest > cx) {
        outWidth = std::max<UINT>(1, static_cast<UINT>(static_cast<ULONGLONG>(width) * cx / longest));
        outHeight = std::max<UINT>(1, static_cast<UINT>(static_cast<ULONGLONG>(height) * cx / longest));
    }
    ComPtr<IWICBitmapScaler> scaler;
    hr = factory->CreateBitmapScaler(&scaler);
    if (FAILED(hr)) return hr;
    hr = scaler->Initialize(frame.Get(), outWidth, outHeight, WICBitmapInterpolationModeFant);
    if (FAILED(hr)) return hr;
    ComPtr<IWICFormatConverter> converter;
    hr = factory->CreateFormatConverter(&converter);
    if (FAILED(hr)) return hr;
    hr = converter->Initialize(scaler.Get(), GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, nullptr, 0, WICBitmapPaletteTypeCustom);
    if (FAILED(hr)) return hr;
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = static_cast<LONG>(outWidth);
    info.bmiHeader.biHeight = -static_cast<LONG>(outHeight);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    void* pixels = nullptr;
    HBITMAP bitmap = CreateDIBSection(nullptr, &info, DIB_RGB_COLORS, &pixels, nullptr, 0);
    if (!bitmap) { const DWORD error = GetLastError(); return error ? HRESULT_FROM_WIN32(error) : E_OUTOFMEMORY; }
    const UINT stride = outWidth * 4;
    hr = converter->CopyPixels(nullptr, stride, stride * outHeight, static_cast<BYTE*>(pixels));
    if (FAILED(hr)) { DeleteObject(bitmap); return hr; }
    *output = bitmap;
    return S_OK;
}

class Thumbnail final : public IThumbnailProvider, public IInitializeWithStream, public IInitializeWithFile {
    std::atomic<ULONG> refs_{1};
    ComPtr<IStream> stream_;
public:
    Thumbnail() noexcept { ++activeObjects; }
    ~Thumbnail() { --activeObjects; }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** output) override {
        if (!output) return E_POINTER;
        *output = nullptr;
        if (iid == IID_IUnknown || iid == IID_IThumbnailProvider) *output = static_cast<IThumbnailProvider*>(this);
        else if (iid == __uuidof(IInitializeWithStream)) *output = static_cast<IInitializeWithStream*>(this);
        else if (iid == __uuidof(IInitializeWithFile)) *output = static_cast<IInitializeWithFile*>(this);
        else return E_NOINTERFACE;
        AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs_; }
    ULONG STDMETHODCALLTYPE Release() override { const ULONG left = --refs_; if (!left) delete this; return left; }
    HRESULT STDMETHODCALLTYPE Initialize(IStream* stream, DWORD) override {
        if (!stream) return E_INVALIDARG;
        if (stream_) return HRESULT_FROM_WIN32(ERROR_ALREADY_INITIALIZED);
        ComPtr<IStream> cloned;
        if (SUCCEEDED(stream->Clone(&cloned))) stream_ = cloned;
        else stream_ = stream;
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE Initialize(LPCWSTR path, DWORD) override {
        if (!path || !*path) return E_INVALIDARG;
        if (stream_) return HRESULT_FROM_WIN32(ERROR_ALREADY_INITIALIZED);
        ComPtr<IStream> stream;
        const HRESULT hr = SHCreateStreamOnFileEx(path, STGM_READ | STGM_SHARE_DENY_NONE, FILE_ATTRIBUTE_NORMAL, FALSE, nullptr, &stream);
        return FAILED(hr) ? hr : Initialize(stream.Get(), STGM_READ);
    }
    HRESULT STDMETHODCALLTYPE GetThumbnail(UINT cx, HBITMAP* bitmap, WTS_ALPHATYPE* alpha) override {
        if (bitmap) *bitmap = nullptr;
        if (alpha) *alpha = WTSAT_UNKNOWN;
        if (!bitmap || !alpha) return E_POINTER;
        if (!cx || cx > 4096) return E_INVALIDARG;
        if (!stream_) return E_UNEXPECTED;
        try {
            std::wstring digest, path;
            HRESULT hr = HashOriginalStream(stream_.Get(), digest);
            if (FAILED(hr)) return hr;
            hr = CachePath(digest, path);
            if (FAILED(hr)) return hr;
            hr = CachedPngBitmap(path, cx, bitmap);
            if (SUCCEEDED(hr)) *alpha = WTSAT_ARGB;
            return hr;
        } catch (const std::bad_alloc&) { return E_OUTOFMEMORY; }
        catch (...) { return E_FAIL; }
    }
};

class Factory final : public IClassFactory {
    std::atomic<ULONG> refs_{1};
public:
    Factory() noexcept { ++activeObjects; }
    ~Factory() { --activeObjects; }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** output) override {
        if (!output) return E_POINTER;
        *output = nullptr;
        if (iid != IID_IUnknown && iid != IID_IClassFactory) return E_NOINTERFACE;
        *output = static_cast<IClassFactory*>(this); AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs_; }
    ULONG STDMETHODCALLTYPE Release() override { const ULONG left = --refs_; if (!left) delete this; return left; }
    HRESULT STDMETHODCALLTYPE CreateInstance(IUnknown* outer, REFIID iid, void** output) override {
        if (!output) return E_POINTER;
        *output = nullptr;
        if (outer) return CLASS_E_NOAGGREGATION;
        auto* instance = new (std::nothrow) Thumbnail();
        if (!instance) return E_OUTOFMEMORY;
        const HRESULT hr = instance->QueryInterface(iid, output); instance->Release(); return hr;
    }
    HRESULT STDMETHODCALLTYPE LockServer(BOOL lock) override {
        if (lock) { ++serverLocks; return S_OK; }
        long count = serverLocks.load();
        while (count > 0) { if (serverLocks.compare_exchange_weak(count, count - 1)) return S_OK; }
        return E_UNEXPECTED;
    }
};
} // namespace

extern "C" HRESULT __stdcall DllGetClassObject(REFCLSID clsid, REFIID iid, void** output) {
    if (!output) return E_POINTER;
    *output = nullptr;
    if (clsid != CLSID_NovaThumbnail) return CLASS_E_CLASSNOTAVAILABLE;
    auto* factory = new (std::nothrow) Factory();
    if (!factory) return E_OUTOFMEMORY;
    const HRESULT hr = factory->QueryInterface(iid, output); factory->Release(); return hr;
}
extern "C" HRESULT __stdcall DllCanUnloadNow() { return activeObjects.load() == 0 && serverLocks.load() == 0 ? S_OK : S_FALSE; }
