#pragma once
#include <windows.h>
#include <string>

// NSIS ExDLL Unicode ABI: https://nsis.sourceforge.io/Docs/AppendixD.html
// The host passes GlobalAlloc-backed linked stack entries and UTF-16 strings.
struct stack_t { stack_t* next; wchar_t text[1]; };
struct extra_parameters;
struct Stack {
    int capacity;
    stack_t** top;
    std::wstring pop() const {
        if (!top || !*top) return {};
        stack_t* item = *top;
        std::wstring value(item->text);
        *top = item->next;
        GlobalFree(item);
        return value;
    }
    void push(const std::wstring& value) const {
        if (!top || capacity < 1) return;
        auto* item = static_cast<stack_t*>(GlobalAlloc(GPTR, sizeof(stack_t) + capacity * sizeof(wchar_t)));
        if (!item) return;
        lstrcpynW(item->text, value.c_str(), capacity);
        item->next = *top;
        *top = item;
    }
};
static_assert(sizeof(void*) == 4, "The bundled NSIS Unicode host is x86, including for an x64 application");
static_assert(sizeof(wchar_t) == 2, "NSIS Unicode ABI requires UTF-16");
#define NSIS_ENTRY(name) extern "C" void __cdecl name(HWND parent, int stringSize, wchar_t*, stack_t** stackTop, extra_parameters*)

// The NSIS host supplies its selected/persisted language before each user-facing call.
inline bool novaInstallerEnglish = false;
inline const wchar_t* novaText(const wchar_t* zh, const wchar_t* en) { return novaInstallerEnglish ? en : zh; }
