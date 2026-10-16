#include "nsis_abi.h"
#include <UIlib.h>
#include <algorithm>
#include <memory>
#include <vector>
#include <commctrl.h>

using namespace DuiLib;
namespace {
HINSTANCE module;
constexpr DWORD background = 0xff0b1725;
constexpr DWORD surface = 0xff14263b;
constexpr DWORD cyan = 0xff42d5f5;
constexpr DWORD foreground = 0xffe8f1fc;

std::wstring textOf(HWND hwnd) {
    const int size = GetWindowTextLengthW(hwnd);
    std::wstring text(static_cast<size_t>(size) + 1, L'\0');
    GetWindowTextW(hwnd, text.data(), size + 1);
    text.resize(size);
    return text;
}
class Icon final : public CControlUI {
public:
    HICON icon = nullptr;
    void PaintStatusImage(HDC dc) override {
        if (!icon) return;
        RECT r = GetPos();
        const int edge = (std::min<LONG>)({r.right - r.left - 16, r.bottom - r.top - 16, 128L});
        if (edge > 0) DrawIconEx(dc, r.left + (r.right - r.left - edge) / 2,
            r.top + (r.bottom - r.top - edge) / 2, icon, edge, edge, 0, nullptr, DI_NORMAL);
    }
};
class Progress final : public CProgressUI {
public:
    void PaintStatusImage(HDC dc) override {
        RECT r = GetPos();
        const int extent = GetMaxValue() - GetMinValue();
        if (extent <= 0) return;
        r.right = r.left + MulDiv(r.right - r.left, GetValue() - GetMinValue(), extent);
        HBRUSH brush = CreateSolidBrush(RGB(66, 213, 245));
        FillRect(dc, &r, brush);
        DeleteObject(brush);
    }
};
struct Mirror { HWND native; CControlUI* control; std::wstring kind; bool seen = false; };
class Surface final : public CWindowWnd, public INotifyUI {
public:
    HWND parent = nullptr;
    CPaintManagerUI paint;
    CContainerUI* root = nullptr;
    std::vector<Mirror> controls;
    bool syncing = false;

    LPCTSTR GetWindowClassName() const override { return L"NovaDuilibNsisSurface"; }
    void Notify(TNotifyUI& event) override {
        if (syncing) return;
        const auto found = std::find_if(controls.begin(), controls.end(), [&](const Mirror& m) { return m.control == event.pSender; });
        if (found == controls.end() || !IsWindow(found->native)) return;
        if (event.sType == L"click") {
            if (found->kind == L"Button") ::PostMessageW(found->native, BM_CLICK, 0, 0);
        } else if (event.sType == L"textchanged" && found->kind == L"Edit") {
            SetWindowTextW(found->native, event.pSender->GetText());
        }
    }
    static BOOL CALLBACK enumerate(HWND hwnd, LPARAM value) {
        auto* self = reinterpret_cast<Surface*>(value);
        if (hwnd == self->m_hWnd || IsChild(self->m_hWnd, hwnd)) return TRUE;
        if (!IsWindowVisible(hwnd)) return TRUE;
        wchar_t name[80]{};
        GetClassNameW(hwnd, name, 80);
        const std::wstring kind(name);
        if (kind == L"#32770" || kind == L"msctls_updown32") return TRUE;
        self->update(hwnd, kind);
        return TRUE;
    }
    void update(HWND native, const std::wstring& kind) {
        const LONG_PTR style = GetWindowLongPtrW(native, GWL_STYLE);
        const int id = GetDlgCtrlID(native);
        auto found = std::find_if(controls.begin(), controls.end(), [&](const Mirror& m) { return m.native == native; });
        if (found == controls.end()) {
            CControlUI* control = nullptr;
            if (kind == L"Button" && (style & BS_TYPEMASK) == BS_GROUPBOX) {
                auto* label = new CTextUI;
                label->SetTextColor(foreground);
                label->SetTextStyle(DT_LEFT);
                label->SetMouseEnabled(false);
                control = label;
            } else if (kind == L"Button") {
                if ((style & BS_TYPEMASK) == BS_AUTOCHECKBOX || (style & BS_TYPEMASK) == BS_CHECKBOX ||
                    (style & BS_TYPEMASK) == BS_AUTORADIOBUTTON || (style & BS_TYPEMASK) == BS_RADIOBUTTON) {
                    auto* option = new CCheckBoxUI;
                    option->SetSelectedBkColor(0xff21445e);
                    option->SetSelectedTextColor(cyan);
                    control = option;
                } else control = new CButtonUI;
                auto* button = static_cast<CButtonUI*>(control);
                button->SetBkColor(id == IDOK ? 0xff126f91 : surface);
                button->SetHotBkColor(0xff255274);
                button->SetTextColor(foreground);
                button->SetHotTextColor(0xffffffff);
                button->SetBorderColor(0xff2c6687);
                button->SetBorderSize(1);
                button->SetBorderRound({6, 6});
            } else if (kind == L"Edit") {
                auto* edit = new CEditUI;
                const auto nativeLimit = ::SendMessageW(native, EM_GETLIMITTEXT, 0, 0);
                edit->SetMaxChar(static_cast<UINT>(nativeLimit > 0 ? (std::min<LRESULT>)(nativeLimit, 32767) : 32767));
                edit->SetBkColor(surface);
                edit->SetTextColor(foreground);
                edit->SetReadOnly((style & ES_READONLY) != 0);
                edit->SetBorderSize(1);
                edit->SetBorderColor(0xff2c6687);
                edit->SetTextPadding({6, 2, 6, 2});
                control = edit;
            } else if (kind == PROGRESS_CLASSW) {
                auto* progress = new Progress;
                progress->SetBkColor(surface);
                control = progress;
            } else if (kind == WC_LISTVIEWW || kind.find(L"RichEdit") != std::wstring::npos || kind.find(L"RICHEDIT") != std::wstring::npos) {
                auto* rich = new CRichEditUI;
                rich->SetReadOnly(true);
                rich->SetRich(false);
                rich->SetBkColor(surface);
                rich->SetTextColor(foreground);
                rich->SetAttribute(L"vscrollbar", L"true");
                rich->SetTextPadding({6, 6, 6, 6});
                control = rich;
            } else if (kind == L"Static" && ((style & SS_TYPEMASK) == SS_BITMAP || (style & SS_TYPEMASK) == SS_ICON)) {
                auto* logo = new Icon;
                logo->icon = reinterpret_cast<HICON>(::SendMessageW(parent, WM_GETICON, ICON_BIG, 0));
                if (!logo->icon) logo->icon = reinterpret_cast<HICON>(GetClassLongPtrW(parent, GCLP_HICON));
                logo->SetBkColor(surface);
                control = logo;
            } else if (kind == L"Static") {
                auto* label = new CTextUI;
                label->SetTextColor(foreground);
                label->SetTextStyle(DT_LEFT | DT_WORDBREAK);
                label->SetMouseEnabled(false);
                control = label;
            } else return;
            control->SetFloat(true);
            root->Add(control);
            controls.push_back({native, control, kind});
            found = controls.end() - 1;
        }
        found->seen = true;
        CControlUI* control = found->control;
        RECT r{};
        GetWindowRect(native, &r);
        MapWindowPoints(nullptr, m_hWnd, reinterpret_cast<POINT*>(&r), 2);
        control->SetFixedXY({r.left, r.top});
        control->SetFixedWidth(r.right - r.left);
        control->SetFixedHeight(r.bottom - r.top);
        control->SetPos(r);
        control->SetVisible(true);
        control->SetEnabled(IsWindowEnabled(native) != FALSE);
        if (kind == PROGRESS_CLASSW) {
            PBRANGE range{};
            ::SendMessageW(native, PBM_GETRANGE, FALSE, reinterpret_cast<LPARAM>(&range));
            auto* progress = static_cast<CProgressUI*>(control);
            progress->SetMinValue(range.iLow);
            progress->SetMaxValue((std::max)(range.iHigh, range.iLow + 1));
            progress->SetValue(static_cast<int>(::SendMessageW(native, PBM_GETPOS, 0, 0)));
        } else if (kind == L"Button" && ((style & BS_TYPEMASK) == BS_AUTOCHECKBOX || (style & BS_TYPEMASK) == BS_CHECKBOX ||
                    (style & BS_TYPEMASK) == BS_AUTORADIOBUTTON || (style & BS_TYPEMASK) == BS_RADIOBUTTON)) {
            static_cast<CCheckBoxUI*>(control)->Selected(::SendMessageW(native, BM_GETCHECK, 0, 0) == BST_CHECKED, false);
        }
        std::wstring text;
        if (kind == WC_LISTVIEWW) {
            const int count = ListView_GetItemCount(native);
            for (int i = (std::max)(0, count - 12); i < count; ++i) {
                wchar_t line[2048]{};
                LVITEMW item{}; item.iSubItem = 0; item.cchTextMax = 2048; item.pszText = line;
                ::SendMessageW(native, LVM_GETITEMTEXTW, i, reinterpret_cast<LPARAM>(&item));
                if (!text.empty()) text += L"\r\n";
                text += line;
            }
        } else text = textOf(native);
        if (kind == L"Button" && ((style & BS_TYPEMASK) == BS_AUTOCHECKBOX || (style & BS_TYPEMASK) == BS_CHECKBOX)) {
            text = (::SendMessageW(native, BM_GETCHECK, 0, 0) == BST_CHECKED ? L"☑ " : L"☐ ") + text;
        } else if (kind == L"Button" && ((style & BS_TYPEMASK) == BS_AUTORADIOBUTTON || (style & BS_TYPEMASK) == BS_RADIOBUTTON)) {
            text = (::SendMessageW(native, BM_GETCHECK, 0, 0) == BST_CHECKED ? L"● " : L"○ ") + text;
        }
        if (kind != PROGRESS_CLASSW && std::wstring(control->GetText().GetData()) != text) control->SetText(text.c_str());
    }
    void sync() {
        if (syncing || !IsWindow(parent)) return;
        syncing = true;
        RECT r{}; GetClientRect(parent, &r);
        SetWindowPos(m_hWnd, HWND_TOP, 0, 0, r.right, r.bottom, SWP_NOACTIVATE);
        for (auto& m : controls) m.seen = false;
        EnumChildWindows(parent, enumerate, reinterpret_cast<LPARAM>(this));
        for (auto& m : controls) if (!m.seen) m.control->SetVisible(false);
        syncing = false;
    }
    LRESULT HandleMessage(UINT message, WPARAM wparam, LPARAM lparam) override {
        if (message == WM_CREATE) {
            paint.Init(m_hWnd);
            paint.SetDefaultFont(L"Microsoft YaHei UI", 9, false, false, false);
            paint.SetDefaultFontColor(foreground);
            paint.SetDefaultDisabledColor(0xff7390aa);
            root = new CContainerUI;
            root->SetBkColor(background);
            paint.AttachDialog(root);
            paint.AddNotifier(this);
            SetTimer(m_hWnd, 1, 60, nullptr);
            return 0;
        }
        if (message == WM_TIMER) { sync(); return 0; }
        if (message == WM_KEYDOWN && (wparam == VK_RETURN || wparam == VK_ESCAPE)) {
            const int id = wparam == VK_ESCAPE ? IDCANCEL : IDOK;
            if (IsWindowEnabled(GetDlgItem(parent, id))) ::PostMessageW(parent, WM_COMMAND, id, 0);
            return 0;
        }
        if (message == WM_DESTROY) KillTimer(m_hWnd, 1);
        LRESULT result{};
        if (paint.MessageHandler(message, wparam, lparam, result)) return result;
        return CWindowWnd::HandleMessage(message, wparam, lparam);
    }
};
std::unique_ptr<Surface> window;
}

NSIS_ENTRY(Attach) {
    Stack stack{stringSize, stackTop};
    const auto title = stack.pop();
    if (!IsWindow(parent)) { stack.push(L"missing NSIS parent"); return; }
    if (!window) {
        CPaintManagerUI::SetInstance(module);
        window = std::make_unique<Surface>();
        window->parent = parent;
        if (!window->Create(parent, title.c_str(), WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                WS_EX_CONTROLPARENT, 0, 0, 1, 1)) {
            window.reset(); stack.push(L"Duilib window creation failed"); return;
        }
        SetWindowTextW(parent, title.c_str());
        window->sync();
    }
    stack.push(L"ok");
}
NSIS_ENTRY(Detach) {
    (void)parent; (void)stringSize; (void)stackTop;
    if (window) { if (IsWindow(window->GetHWND())) DestroyWindow(window->GetHWND()); window.reset(); }
}
NSIS_ENTRY(Abi) { (void)parent; Stack{stringSize, stackTop}.push(L"NovaDui/1;x86;Unicode;pointer=4;wchar=2;static-crt;duilib=85658d47036445626f5a24d5287f348fff871604"); }
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) { module = instance; DisableThreadLibraryCalls(instance); }
    return TRUE;
}
