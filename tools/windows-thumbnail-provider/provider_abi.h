#pragma once
#include <windows.h>
// Stable public COM class, registered by the Nova installer, never by this DLL.
inline constexpr CLSID CLSID_NovaThumbnail = {0x95331dae, 0x48dc, 0x4bbd, {0xa4, 0xe0, 0xe0, 0x88, 0x0d, 0x36, 0x58, 0x70}};
inline constexpr wchar_t NovaThumbnailClsid[] = L"{95331DAE-48DC-4BBD-A4E0-E0880D365870}";
