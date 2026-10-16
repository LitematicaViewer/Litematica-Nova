#include "provider_abi.h"
#include <thumbcache.h>
#include <propsys.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <wincodec.h>
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

struct StreamPosition {
    IStream* stream;
    ULARGE_INTEGER original{};
    bool known = false;
    explicit StreamPosition(IStream* input) : stream(input) { LARGE_INTEGER zero{}; known = SUCCEEDED(stream->Seek(zero, STREAM_SEEK_CUR, &original)); }
    ~StreamPosition() { if (known && original.QuadPart <= static_cast<ULONGLONG>(LLONG_MAX)) { LARGE_INTEGER position{}; position.QuadPart = static_cast<LONGLONG>(original.QuadPart); stream->Seek(position, STREAM_SEEK_SET, nullptr); } }
};

struct Handle {
    HANDLE value = nullptr;
    Handle() = default;
    explicit Handle(HANDLE handle) : value(handle) {}
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    ~Handle() { if (value && value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};
struct ChildProcess {
    Handle process;
    Handle thread;
    ~ChildProcess() {
        if (process.value && WaitForSingleObject(process.value, 0) == WAIT_TIMEOUT) {
            TerminateProcess(process.value, ERROR_CANCELLED);
            WaitForSingleObject(process.value, 5000);
        }
    }
};
struct TemporaryPng {
    std::wstring path;
    ~TemporaryPng() { if (!path.empty()) DeleteFileW(path.c_str()); }
    HRESULT Create() {
        std::array<wchar_t, 32768> folder{};
        const DWORD size = GetTempPathW(static_cast<DWORD>(folder.size()), folder.data());
        if (!size || size >= folder.size()) return HRESULT_FROM_WIN32(ERROR_BAD_PATHNAME);
        std::array<wchar_t, MAX_PATH> reserved{};
        if (!GetTempFileNameW(folder.data(), L"NVT", 0, reserved.data())) return HRESULT_FROM_WIN32(GetLastError());
        path = reserved.data();
        const std::wstring png = path + L".png";
        if (!MoveFileW(path.c_str(), png.c_str())) return HRESULT_FROM_WIN32(GetLastError());
        path = png;
        return S_OK;
    }
};
struct AttributeList {
    std::vector<BYTE> bytes;
    LPPROC_THREAD_ATTRIBUTE_LIST value = nullptr;
    ~AttributeList() { if (value) DeleteProcThreadAttributeList(value); }
};

HRESULT EmbeddedPreview(IStream* stream, const std::wstring& output) {
    // Resolve only the CLI shipped beside this provider. Never search PATH or start the UI/Viewer.
    HMODULE module = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(&activeObjects), &module)) return HRESULT_FROM_WIN32(GetLastError());
    std::array<wchar_t, 32768> modulePath{};
    const DWORD moduleSize = GetModuleFileNameW(module, modulePath.data(), static_cast<DWORD>(modulePath.size()));
    if (!moduleSize || moduleSize >= modulePath.size()) return HRESULT_FROM_WIN32(ERROR_BAD_PATHNAME);
    std::wstring root(modulePath.data(), moduleSize);
    const auto separator = root.find_last_of(L"\\/");
    if (separator == std::wstring::npos) return HRESULT_FROM_WIN32(ERROR_BAD_PATHNAME);
    root.resize(separator);
    const std::wstring core = root + L"\\bin\\viewer-backend\\litematica_core.exe";
    const DWORD coreAttributes = GetFileAttributesW(core.c_str());
    if (coreAttributes == INVALID_FILE_ATTRIBUTES || (coreAttributes & FILE_ATTRIBUTE_DIRECTORY)) return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);

    static std::atomic<unsigned long> sequence{0};
    const std::wstring pipeName = L"\\\\.\\pipe\\nova-embedded-preview-" + std::to_wstring(GetCurrentProcessId()) + L"-" + std::to_wstring(++sequence);
    Handle write(CreateNamedPipeW(pipeName.c_str(), PIPE_ACCESS_OUTBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1, 64 * 1024, 64 * 1024, 0, nullptr));
    if (write.value == INVALID_HANDLE_VALUE) return HRESULT_FROM_WIN32(GetLastError());
    SECURITY_ATTRIBUTES security{sizeof(SECURITY_ATTRIBUTES), nullptr, TRUE};
    Handle read(CreateFileW(pipeName.c_str(), GENERIC_READ, 0, &security, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr));
    if (read.value == INVALID_HANDLE_VALUE) return HRESULT_FROM_WIN32(GetLastError());
    Handle connected(CreateEventW(nullptr, TRUE, FALSE, nullptr));
    if (!connected.value) return HRESULT_FROM_WIN32(GetLastError());
    OVERLAPPED connection{}; connection.hEvent = connected.value;
    if (!ConnectNamedPipe(write.value, &connection) && GetLastError() != ERROR_PIPE_CONNECTED) return HRESULT_FROM_WIN32(GetLastError());
    Handle nullOutput(CreateFileW(L"NUL", GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, &security, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr));
    if (nullOutput.value == INVALID_HANDLE_VALUE) return HRESULT_FROM_WIN32(GetLastError());

    AttributeList attributes;
    SIZE_T attributeBytes = 0;
    InitializeProcThreadAttributeList(nullptr, 1, 0, &attributeBytes);
    if (!attributeBytes) return HRESULT_FROM_WIN32(GetLastError());
    attributes.bytes.resize(attributeBytes);
    auto* list = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(attributes.bytes.data());
    if (!InitializeProcThreadAttributeList(list, 1, 0, &attributeBytes)) return HRESULT_FROM_WIN32(GetLastError());
    attributes.value = list;
    HANDLE inherited[] = {read.value, nullOutput.value};
    if (!UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, inherited, sizeof(inherited), nullptr, nullptr)) return HRESULT_FROM_WIN32(GetLastError());
    STARTUPINFOEXW startup{};
    startup.StartupInfo.cb = sizeof(startup);
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    startup.StartupInfo.wShowWindow = SW_HIDE;
    startup.StartupInfo.hStdInput = read.value;
    startup.StartupInfo.hStdOutput = nullOutput.value;
    startup.StartupInfo.hStdError = nullOutput.value;
    startup.lpAttributeList = list;
    PROCESS_INFORMATION process{};
    std::wstring command = L"\"" + core + L"\" projection-preview --stdin --output \"" + output + L"\"";
    if (!CreateProcessW(core.c_str(), command.data(), nullptr, nullptr, TRUE, CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
        nullptr, root.c_str(), &startup.StartupInfo, &process)) return HRESULT_FROM_WIN32(GetLastError());
    ChildProcess child;
    child.process.value = process.hProcess; child.thread.value = process.hThread;
    CloseHandle(read.value); read.value = nullptr;

    StreamPosition restore(stream);
    LARGE_INTEGER zero{};
    HRESULT hr = stream->Seek(zero, STREAM_SEEK_SET, nullptr);
    if (FAILED(hr)) return hr;
    const ULONGLONG deadline = GetTickCount64() + 15000;
    Handle completed(CreateEventW(nullptr, TRUE, FALSE, nullptr));
    if (!completed.value) return HRESULT_FROM_WIN32(GetLastError());
    std::array<BYTE, 64 * 1024> buffer{};
    while (true) {
        ULONG count = 0;
        hr = stream->Read(buffer.data(), static_cast<ULONG>(buffer.size()), &count);
        if (FAILED(hr)) return hr;
        if (count > buffer.size()) return E_UNEXPECTED;
        ULONG offset = 0;
        while (offset < count) {
            const ULONGLONG now = GetTickCount64();
            if (now >= deadline) return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
            ResetEvent(completed.value);
            OVERLAPPED pending{}; pending.hEvent = completed.value;
            DWORD written = 0;
            if (!WriteFile(write.value, buffer.data() + offset, count - offset, &written, &pending)) {
                const DWORD error = GetLastError();
                if (error != ERROR_IO_PENDING) return HRESULT_FROM_WIN32(error);
                HANDLE waits[] = {completed.value, child.process.value};
                const DWORD waited = WaitForMultipleObjects(2, waits, FALSE, static_cast<DWORD>(deadline - now));
                if (waited != WAIT_OBJECT_0) {
                    CancelIoEx(write.value, &pending);
                    GetOverlappedResult(write.value, &pending, &written, TRUE);
                    return HRESULT_FROM_WIN32(waited == WAIT_TIMEOUT ? ERROR_TIMEOUT : ERROR_BROKEN_PIPE);
                }
                if (!GetOverlappedResult(write.value, &pending, &written, FALSE)) return HRESULT_FROM_WIN32(GetLastError());
            }
            if (!written) return HRESULT_FROM_WIN32(ERROR_BROKEN_PIPE);
            offset += written;
        }
        if (!count || hr == S_FALSE) break;
    }
    CloseHandle(write.value); write.value = nullptr; // EOF is the input boundary, not a path string.
    const ULONGLONG now = GetTickCount64();
    if (now >= deadline || WaitForSingleObject(child.process.value, static_cast<DWORD>(deadline - now)) != WAIT_OBJECT_0) return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
    DWORD exitCode = 0;
    if (!GetExitCodeProcess(child.process.value, &exitCode)) return HRESULT_FROM_WIN32(GetLastError());
    // No embedded image is a normal thumbnail miss; external cache never overrides that absence.
    if (exitCode != 0) return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
    // The CLI deliberately exits successfully without writing a PNG when a
    // valid projection has no embedded image. The reserved output stays empty.
    WIN32_FILE_ATTRIBUTE_DATA outputInfo{};
    if (!GetFileAttributesExW(output.c_str(), GetFileExInfoStandard, &outputInfo)) return HRESULT_FROM_WIN32(GetLastError());
    if (!outputInfo.nFileSizeHigh && !outputInfo.nFileSizeLow) return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
    return S_OK;
}

HRESULT PngBitmap(const std::wstring& path, UINT cx, HBITMAP* output) {
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
            TemporaryPng image;
            HRESULT hr = image.Create();
            if (FAILED(hr)) return hr;
            hr = EmbeddedPreview(stream_.Get(), image.path);
            if (FAILED(hr)) return hr;
            hr = PngBitmap(image.path, cx, bitmap);
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
