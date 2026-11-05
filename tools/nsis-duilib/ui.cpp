#include "nsis_abi.h"
#include <UIlib.h>
#include <algorithm>
#include <cstdlib>
#include <memory>
#include <vector>
#include <commctrl.h>

using namespace DuiLib;
namespace {
HINSTANCE module;
constexpr UINT_PTR syncTimer = 0x4e53;
constexpr UINT syncPage = WM_APP + 0x4e5;
constexpr wchar_t ownedClip[] = L"NovaDui.addedClipStyle";
// The neutral surfaces, contrasting primary action and single-pixel outlines mirror
// the app's shadcn theme. MUI remains the authority for layout and navigation.
struct Theme {
    DWORD bg, panel, ink, muted, border, primary, onPrimary, hover, pressed, disabled, ring, danger;
};
constexpr Theme light{0xfffafafa,0xffffffff,0xff18181b,0xff52525b,0xffd4d4d8,0xff27272a,0xfffafafa,
                      0xfff1f1f3,0xffe4e4e7,0xff71717a,0xff28678e,0xffad2637};
constexpr Theme dark{0xff111214,0xff1a1b1e,0xfff4f4f5,0xffb5b5bd,0xff414147,0xffe4e4e7,0xff18181b,
                     0xff29292e,0xff34343a,0xff9999a3,0xff89bfe0,0xffff9ba8};
Theme theme = light;
int dpi = 96;
int px(int value) { return MulDiv(value, dpi, 96); }
COLORREF rgb(DWORD c) { return RGB((c >> 16) & 255, (c >> 8) & 255, c & 255); }
bool nativeAccessibility() {
    HIGHCONTRASTW hc{sizeof(hc)};
    BOOL reader = FALSE;
    SystemParametersInfoW(SPI_GETSCREENREADER, 0, &reader, 0);
    return reader || (SystemParametersInfoW(SPI_GETHIGHCONTRAST, sizeof(hc), &hc, 0) && (hc.dwFlags & HCF_HIGHCONTRASTON));
}
bool systemDark() {
    DWORD value = 1, bytes = sizeof(value);
    return RegGetValueW(HKEY_CURRENT_USER,L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        L"AppsUseLightTheme",RRF_RT_REG_DWORD,nullptr,&value,&bytes) == ERROR_SUCCESS && !value;
}
int dialogDpi(HWND hwnd) {
    using GetDpi = UINT (WINAPI*)(HWND);
    static const auto getDpi = reinterpret_cast<GetDpi>(GetProcAddress(GetModuleHandleW(L"user32"), "GetDpiForWindow"));
    if (getDpi) { const auto value = getDpi(hwnd); if (value) return static_cast<int>(value); }
    HDC dc = GetDC(hwnd); const int value = dc ? GetDeviceCaps(dc,LOGPIXELSY) : 96;
    if (dc) ReleaseDC(hwnd,dc);
    return value > 0 ? value : 96;
}
void box(HDC dc, RECT r, DWORD fill, DWORD stroke, int corner) {
    if (IsRectEmpty(&r)) return;
    auto brush = SelectObject(dc, GetStockObject(DC_BRUSH));
    auto pen = SelectObject(dc, GetStockObject(DC_PEN));
    SetDCBrushColor(dc, rgb(fill)); SetDCPenColor(dc, rgb(stroke));
    RoundRect(dc,r.left,r.top,r.right,r.bottom,corner,corner);
    SelectObject(dc,pen); SelectObject(dc,brush);
}
std::wstring textOf(HWND hwnd) {
    const int size = GetWindowTextLengthW(hwnd);
    std::wstring text(static_cast<size_t>(size) + 1, L'\0');
    text.resize(GetWindowTextW(hwnd, text.data(), size + 1));
    return text;
}
bool choiceType(LONG_PTR type) {
    return type == BS_AUTOCHECKBOX || type == BS_CHECKBOX || type == BS_AUTORADIOBUTTON ||
           type == BS_RADIOBUTTON || type == BS_AUTO3STATE || type == BS_3STATE;
}
class Action final : public CButtonUI {
public:
    HWND native = nullptr;
    LONG_PTR type = 0;
    int id = 0;
    LRESULT checked = BST_UNCHECKED;
    bool nativeFocused = false;
    bool primary() const { return id == IDOK || type == BS_DEFPUSHBUTTON; }
    void PaintText(HDC dc) override {
        // Duilib's text renderer always forces DT_NOPREFIX. Use the native accelerator
        // convention so "&Next" / "下一步(&N)" display correctly and Alt reveals the cue.
        RECT r = GetPos(), padding = GetTextPadding(); r.left += padding.left; r.right -= padding.right;
        const int saved = SaveDC(dc);
        SelectObject(dc,m_pManager->GetFont(GetFont()));
        ::SetTextColor(dc,rgb(IsEnabled() ? GetTextColor() : GetDisabledTextColor()));
        SetBkMode(dc,TRANSPARENT);
        UINT format = GetTextStyle();
        if (::SendMessageW(native,WM_QUERYUISTATE,0,0) & UISF_HIDEACCEL) format |= DT_HIDEPREFIX;
        ::DrawTextW(dc,GetText().GetData(),-1,&r,format);
        RestoreDC(dc,saved);
    }
    void PaintStatusImage(HDC dc) override {
        RECT r = GetPos();
        const bool choice = choiceType(type);
        const bool hot = IsEnabled() && (m_uButtonState & UISTATE_HOT);
        const bool down = IsEnabled() && (m_uButtonState & UISTATE_PUSHED);
        const bool focused = IsEnabled() && (nativeFocused || (IsFocused() && ::GetFocus() == m_pManager->GetPaintWindow())) &&
            !(::SendMessageW(native, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS);
        if (focused) { box(dc,r,theme.ring,theme.ring,px(8)); InflateRect(&r,-px(2),-px(2)); }
        if (choice) {
            box(dc,r,(hot || down) ? theme.hover : theme.bg,(hot || down) ? theme.hover : theme.bg,px(6));
            const bool radio = type == BS_RADIOBUTTON || type == BS_AUTORADIOBUTTON;
            const int size = (std::min<LONG>)(px(14),r.bottom-r.top);
            RECT mark{r.left+px(2),r.top+(r.bottom-r.top-size)/2,0,0}; mark.right=mark.left+size; mark.bottom=mark.top+size;
            const DWORD base = IsEnabled() ? theme.primary : theme.disabled;
            box(dc, mark, checked ? base : theme.panel, checked ? base : theme.muted, radio ? size : px(4));
            if (checked == BST_INDETERMINATE || (checked && radio)) {
                InflateRect(&mark,-size/3,-(radio ? size/3 : (size-2)/2));
                box(dc,mark,theme.onPrimary,theme.onPrimary,radio ? size : 0);
            } else if (checked) {
                HPEN pen = CreatePen(PS_SOLID,(std::max)(1,px(2)),rgb(theme.onPrimary));
                auto old = SelectObject(dc,pen);
                MoveToEx(dc,mark.left+size/5,mark.top+size/2,nullptr);
                LineTo(dc,mark.left+size*2/5,mark.top+size*7/10); LineTo(dc,mark.left+size*4/5,mark.top+size*3/10);
                SelectObject(dc,old); DeleteObject(pen);
            }
            return;
        }
        const bool main = IsEnabled() && primary();
        const DWORD fill = !IsEnabled() ? theme.bg : main ? ((hot || down) ? theme.muted : theme.primary) :
            down ? theme.pressed : hot ? theme.hover : (id == 3 ? theme.bg : theme.panel);
        box(dc,r,fill,main ? fill : (id == 3 && !hot && !down ? theme.bg : theme.border),px(7));
    }
};
class Icon final : public CControlUI {
public:
    HICON icon = nullptr;
    void PaintStatusImage(HDC dc) override {
        RECT r = GetPos();
        const bool rail = r.bottom-r.top > px(100);
        if (rail) box(dc,r,theme.panel,theme.border,0);
        const int edge = (std::min<LONG>)({r.right-r.left-px(8),r.bottom-r.top-px(8),px(rail ? 64 : 32)});
        if (icon && edge > 0) DrawIconEx(dc,r.left+(r.right-r.left-edge)/2,r.top+(r.bottom-r.top-edge)/2,
            icon,edge,edge,0,nullptr,DI_NORMAL);
    }
};
class EditFrame final : public CControlUI {
public:
    bool focused = false;
    void PaintStatusImage(HDC dc) override {
        box(dc,GetPos(),theme.panel,focused ? theme.ring : theme.border,px(5));
    }
};
class Progress final : public CProgressUI {
public:
    bool failed = false;
    void PaintStatusImage(HDC dc) override {
        RECT r = GetPos();
        const int h = (std::min<LONG>)(r.bottom-r.top,px(6));
        r.top += (r.bottom-r.top-h)/2; r.bottom = r.top+h;
        box(dc,r,theme.border,theme.border,h);
        const int extent = GetMaxValue()-GetMinValue();
        if (extent <= 0 || GetValue() <= GetMinValue()) return;
        r.right = r.left+MulDiv(r.right-r.left,GetValue()-GetMinValue(),extent);
        const DWORD ink = failed ? theme.danger : theme.primary;
        box(dc,r,ink,ink,h);
    }
};
struct Mirror { HWND native; CControlUI* control; std::wstring kind; LONG_PTR type; int id; bool seen = false; };
void detailsColors(HWND list, bool system) {
    wchar_t kind[32]{};
    if (!IsWindow(list) || GetDlgCtrlID(list)!=1016 || !GetClassNameW(list,kind,32) || lstrcmpiW(kind,WC_LISTVIEWW)) return;
    const COLORREF bg=system ? GetSysColor(COLOR_WINDOW) : rgb(theme.panel);
    const COLORREF ink=system ? GetSysColor(COLOR_WINDOWTEXT) : rgb(theme.ink);
    // Only the ordinary row colors are changed. Selection, scrolling, keyboard
    // handling and accessibility all remain the responsibility of SysListView32.
    if (ListView_GetBkColor(list)!=bg) ListView_SetBkColor(list,bg);
    if (ListView_GetTextBkColor(list)!=bg) ListView_SetTextBkColor(list,bg);
    if (ListView_GetTextColor(list)!=ink) ListView_SetTextColor(list,ink);
}
class Surface final : public CWindowWnd, public INotifyUI {
public:
    HWND parent = nullptr;
    HWND details = nullptr;
    CPaintManagerUI paint;
    CContainerUI* root = nullptr;
    std::vector<Mirror> controls;
    std::vector<RECT> cutouts, lastCutouts;
    SIZE lastSize{-1,-1};
    ULONGLONG nextSettings = 0;
    bool syncing = false, nativeOnly = false, restyle = true, failed = false;
    LPCTSTR GetWindowClassName() const override { return L"NovaDuilibNsisSurface"; }
    // The MUI page and footer/header HWNDs are siblings of this surface. HWND_TOP
    // controls hit testing, but without WS_CLIPSIBLINGS they may still paint OVER
    // it. Clip the actual drawing owners; leave visibility, tab order and IDs alone.
    static void clip(HWND hwnd, LONG_PTR flag, bool enable) {
        const auto style=GetWindowLongPtrW(hwnd,GWL_STYLE);
        if (enable) {
            if ((style & flag) || !SetPropW(hwnd,ownedClip,reinterpret_cast<HANDLE>(flag))) return;
            SetWindowLongPtrW(hwnd,GWL_STYLE,style | flag);
        } else if (RemovePropW(hwnd,ownedClip)) {
            SetWindowLongPtrW(hwnd,GWL_STYLE,style & ~flag);
        }
    }
    static LRESULT CALLBACK hostProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp, UINT_PTR key, DWORD_PTR data) {
        auto* self=reinterpret_cast<Surface*>(data);
        if (hwnd==self->parent && msg==WM_PARENTNOTIFY &&
            (LOWORD(wp)==WM_CREATE || LOWORD(wp)==WM_DESTROY))
            ::PostMessageW(self->m_hWnd,syncPage,0,0);
        if (!self->nativeOnly && (msg==WM_CTLCOLOREDIT || msg==WM_CTLCOLORSTATIC)) {
            HWND child=reinterpret_cast<HWND>(lp);
            wchar_t kind[24]{};
            if (GetClassNameW(child,kind,24) && !lstrcmpiW(kind,L"Edit")) {
                HDC dc=reinterpret_cast<HDC>(wp);
                ::SetTextColor(dc,rgb(IsWindowEnabled(child) ? theme.ink : theme.disabled));
                SetBkColor(dc,rgb(theme.panel));
                SetDCBrushColor(dc,rgb(theme.panel));
                return reinterpret_cast<LRESULT>(GetStockObject(DC_BRUSH));
            }
        }
        if (msg==WM_NCDESTROY) RemoveWindowSubclass(hwnd,hostProc,key);
        return DefSubclassProc(hwnd,msg,wp,lp);
    }
    void restoreNative() {
        clip(parent,WS_CLIPCHILDREN,false);
        for (HWND h=::GetWindow(parent,GW_CHILD); h; h=::GetWindow(h,GW_HWNDNEXT)) {
            if (h==m_hWnd) continue;
            clip(h,WS_CLIPSIBLINGS,false);
            RemoveWindowSubclass(h,hostProc,reinterpret_cast<UINT_PTR>(this));
        }
        if (IsChild(parent,details)) detailsColors(details,true);
        RedrawWindow(parent,nullptr,nullptr,RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN);
    }
    void Notify(TNotifyUI& event) override {
        if (syncing) return;
        const auto found = std::find_if(controls.begin(), controls.end(), [&](const Mirror& m) { return m.control == event.pSender; });
        if (found == controls.end() || !IsWindow(found->native)) return;
        if (event.sType == L"click") {
            if (found->kind == L"Button" && IsWindowEnabled(found->native)) {
                ::SetFocus(found->native); // Tab, Space, mnemonics and screen readers keep the real MUI control.
                const int id = GetDlgCtrlID(found->native);
                const auto type = GetWindowLongPtrW(found->native, GWL_STYLE) & BS_TYPEMASK;
                if (GetParent(found->native) == parent && (id == IDOK || id == IDCANCEL || id == 3) &&
                        (type == BS_PUSHBUTTON || type == BS_DEFPUSHBUTTON)) {
                    // MUI can enter nsDialogs::Show while processing Next. A second
                    // BM_CLICK on that same native button is then reentrant and
                    // ignored. Queue its normal command, outside the button proc.
                    if (IsWindowEnabled(found->native)) ::PostMessageW(parent, WM_COMMAND,
                        MAKEWPARAM(id, BN_CLICKED), reinterpret_cast<LPARAM>(found->native));
                } else ::PostMessageW(found->native, BM_CLICK, 0, 0);
            }
        }
    }
    static BOOL CALLBACK enumerate(HWND hwnd, LPARAM value) {
        auto* self = reinterpret_cast<Surface*>(value);
        if (hwnd == self->m_hWnd || IsChild(self->m_hWnd,hwnd) || !IsWindowVisible(hwnd)) return TRUE;
        if (GetParent(hwnd)==self->parent) clip(hwnd,WS_CLIPSIBLINGS,true);
        wchar_t name[80]{};
        GetClassNameW(hwnd,name,80);
        if (lstrcmpW(name,L"#32770") != 0) self->update(hwnd,name);
        return TRUE;
    }
    void update(HWND native, const std::wstring& kind) {
        const LONG_PTR style = GetWindowLongPtrW(native,GWL_STYLE);
        const LONG_PTR type = kind == L"Button" ? style & BS_TYPEMASK : kind == L"Static" ? style & SS_TYPEMASK : 0;
        const int id = GetDlgCtrlID(native);
        RECT r{}; GetWindowRect(native,&r); MapWindowPoints(nullptr,m_hWnd,reinterpret_cast<POINT*>(&r),2);
        if (IsRectEmpty(&r)) return;
        const bool edit = kind==L"Edit";
        if (edit) {
            // Keep the native edit's entire client (including caret, selection,
            // IME and context menu). Only its nonclient border is drawn by Duilib.
            RECT client{}; GetClientRect(native,&client);
            MapWindowPoints(native,m_hWnd,reinterpret_cast<POINT*>(&client),2);
            cutouts.push_back(client);
            InflateRect(&r,px(1),px(1));
        }
        // Header/branding backgrounds are decorative MUI statics, not logos. Keep the
        // original edit, license, details list, tree, etc. alive, accessible and scrollable.
        if (kind == L"Static" && GetParent(native) == parent) {
            if (id == 1034 || id == 1028) return;
            // The full-window nsDialogs page covers MUI's standard header rule.
            if (id == 1036 && IsWindowVisible(GetDlgItem(parent,1045))) return;
            // MUI places a line behind its branding label. Our labels have transparent
            // backgrounds: put the divider below the actual localized label instead.
            if (id == 1035) {
                HWND branding=GetDlgItem(parent,1256);
                RECT text{};
                if (IsWindowVisible(branding) && GetWindowRect(branding,&text)) {
                    MapWindowPoints(nullptr,m_hWnd,reinterpret_cast<POINT*>(&text),2);
                    r.top=(std::max)(r.top,text.bottom+px(1));
                }
            }
        }
        // nsDialogs sets SS_NOTIFY on ordinary labels and bitmaps, including the
        // entire welcome/finish page. Its actual hyperlinks have class LINK.
        if (!edit && kind != L"Button" && kind != L"Static" && kind != PROGRESS_CLASSW) {
            if (id==1016 && kind==WC_LISTVIEWW &&
                    (native!=details || restyle || ListView_GetBkColor(native)!=rgb(theme.panel))) {
                details=native; detailsColors(details,false);
            }
            cutouts.push_back(r);
            return;
        }
        const bool divider = kind == L"Static" && (type == SS_ETCHEDHORZ || id == 1035 || id == 1045);
        const bool icon = kind == L"Static" && (type == SS_BITMAP || type == SS_ICON);
        const bool button = kind == L"Button" && type != BS_GROUPBOX;
        if (divider) r.bottom = r.top + px(1);
        auto found = std::find_if(controls.begin(),controls.end(),[&](const Mirror& m) {
            return m.native==native && m.kind==kind && m.type==type && m.id==id;
        });
        const bool fresh = found == controls.end();
        if (fresh) {
            CControlUI* control;
            if (edit) {
                control = new EditFrame;
            } else if (button) {
                auto* action = new Action;
                action->native=native; action->id=id; action->type=type; control=action;
            } else if (kind == PROGRESS_CLASSW) {
                control = new Progress;
            } else if (icon) {
                auto* logo = new Icon;
                if (type == SS_ICON) logo->icon = reinterpret_cast<HICON>(::SendMessageW(native,STM_GETICON,0,0));
                if (!logo->icon) logo->icon = reinterpret_cast<HICON>(::SendMessageW(parent,WM_GETICON,ICON_BIG,0));
                if (!logo->icon) logo->icon = reinterpret_cast<HICON>(GetClassLongPtrW(parent,GCLP_HICON));
                control=logo;
            } else if (divider) {
                control = new CControlUI;
            } else {
                control = new CTextUI;
            }
            if (!button) control->SetMouseEnabled(false);
            control->SetFloat(true); root->Add(control);
            controls.push_back({native,control,kind,type,id}); found = controls.end()-1;
        }
        found->seen = true;
        CControlUI* control = found->control;
        const bool styleNow = fresh || restyle;
        const RECT current = control->GetPos();
        if (fresh || !EqualRect(&current,&r)) {
            control->SetFixedXY({r.left,r.top});
            control->SetFixedWidth(r.right-r.left); control->SetFixedHeight(r.bottom-r.top);
            control->SetPos(r);
        }
        if (!control->IsVisible()) control->SetVisible(true);
        const bool enabled = IsWindowEnabled(native)!=FALSE;
        if (control->IsEnabled()!=enabled) control->SetEnabled(enabled);
        if (edit) {
            auto* frame=static_cast<EditFrame*>(control);
            const bool focused=::GetFocus()==native;
            if (styleNow) {
                SetWindowSubclass(GetParent(native),hostProc,reinterpret_cast<UINT_PTR>(this),reinterpret_cast<DWORD_PTR>(this));
                InvalidateRect(native,nullptr,TRUE);
            }
            if (styleNow || focused!=frame->focused) { frame->focused=focused; frame->Invalidate(); }
            return;
        } else if (button) {
            auto* action = static_cast<Action*>(control);
            const bool choice = choiceType(type);
            const LRESULT check = choice ? ::SendMessageW(native,BM_GETCHECK,0,0) : BST_UNCHECKED;
            const bool focused = ::GetFocus()==native;
            if (focused != action->nativeFocused || check != action->checked) {
                action->nativeFocused=focused; action->checked=check; action->Invalidate();
            }
            if (styleNow) {
                action->SetTextColor(choice || !action->primary() ? theme.ink : theme.onPrimary);
                action->SetDisabledTextColor(theme.disabled);
                action->SetTextStyle((choice ? DT_LEFT : DT_CENTER) | DT_SINGLELINE | DT_VCENTER);
                action->SetTextPadding({choice ? px(23) : 0,0,0,0});
            }
        } else if (kind == PROGRESS_CLASSW) {
            PBRANGE range{};
            ::SendMessageW(native,PBM_GETRANGE,FALSE,reinterpret_cast<LPARAM>(&range));
            auto* progress=static_cast<Progress*>(control);
            const int max=(std::max)(range.iHigh,range.iLow+1);
            const int pos=static_cast<int>(::SendMessageW(native,PBM_GETPOS,0,0));
            if (progress->GetMinValue()!=range.iLow) progress->SetMinValue(range.iLow);
            if (progress->GetMaxValue()!=max) progress->SetMaxValue(max);
            if (progress->GetValue()!=pos) progress->SetValue(pos);
            const bool error=failed || ::SendMessageW(native,PBM_GETSTATE,0,0)==PBST_ERROR;
            if (error!=progress->failed || styleNow) { progress->failed=error; progress->Invalidate(); }
            return;
        } else if (icon || divider) {
            if (styleNow) { if (divider) control->SetBkColor(theme.border); control->Invalidate(); }
            return;
        } else if (styleNow) {
            auto* label = static_cast<CTextUI*>(control);
            LOGFONTW font{};
            auto hfont = reinterpret_cast<HFONT>(::SendMessageW(native,WM_GETFONT,0,0));
            if (hfont) GetObjectW(hfont,sizeof(font),&font);
            const bool header = id==1037 || font.lfWeight>=FW_SEMIBOLD || kind==L"Button";
            const bool danger = failed && (id==1037 || id==1006);
            const int fontId = header ? ((r.bottom-r.top >= px(25) && std::abs(font.lfHeight)>=px(15)) ? 2 : 1) :
                               (id==1256 ? 3 : -1);
            label->SetFont(fontId);
            label->SetTextColor(danger ? theme.danger : (id==1038 || id==1256) ? theme.muted : theme.ink);
            label->SetTextStyle((type==SS_CENTER ? DT_CENTER : type==SS_RIGHT ? DT_RIGHT : DT_LEFT) |
                ((style & SS_NOPREFIX) ? DT_NOPREFIX : 0) | DT_WORDBREAK);
        }
        const auto text=textOf(native);
        if (std::wstring(control->GetText().GetData())!=text) control->SetText(text.c_str());
    }
    void shape(const RECT& bounds) {
        if (bounds.right==lastSize.cx && bounds.bottom==lastSize.cy && cutouts.size()==lastCutouts.size() &&
            std::equal(cutouts.begin(),cutouts.end(),lastCutouts.begin(),
                [](const RECT& a,const RECT& b) { return EqualRect(&a,&b)!=FALSE; })) return;
        HRGN visible = CreateRectRgn(0,0,bounds.right,bounds.bottom);
        if (!visible) return;
        for (const RECT& hole : cutouts) {
            HRGN item=CreateRectRgnIndirect(&hole);
            if (!item) { DeleteObject(visible); return; }
            int result=CombineRgn(visible,visible,item,RGN_DIFF); DeleteObject(item);
            if (result==ERROR) { DeleteObject(visible); return; }
        }
        if (!SetWindowRgn(m_hWnd,visible,TRUE)) { DeleteObject(visible); return; }
        lastCutouts=cutouts; lastSize={bounds.right,bounds.bottom}; // HWND owns the installed region.
    }
    void sync() {
        if (syncing || !IsWindow(parent)) return;
        syncing = true;
        const auto now=::GetTickCount64();
        if (now>=nextSettings || nextSettings==0) {
            nextSettings=now+1000;
            const bool native=nativeAccessibility(); const Theme next=systemDark() ? dark : light;
            if (native!=nativeOnly) {
                nativeOnly=native;
                ::ShowWindow(m_hWnd,native ? SW_HIDE : SW_SHOWNOACTIVATE);
                if (native) restoreNative();
                restyle=true;
            }
            const int effectiveDpi=dialogDpi(parent);
            if (restyle || theme.bg!=next.bg || dpi!=effectiveDpi) {
                theme=next; dpi=effectiveDpi; restyle=true;
                // Duilib takes physical pixel heights, not typographic points.
                const auto family=novaInstallerEnglish ? L"Segoe UI" : L"Microsoft YaHei UI";
                paint.SetDefaultFont(family,px(12),false,false,false);
                for (int id=1;id<=3;++id) paint.RemoveFont(id);
                paint.AddFont(1,family,px(13),true,false,false);
                paint.AddFont(2,family,px(17),true,false,false);
                paint.AddFont(3,family,px(11),false,false,false);
                paint.SetDefaultFontColor(theme.ink); paint.SetDefaultDisabledColor(theme.disabled);
                root->SetBkColor(theme.bg);
            }
        }
        if (!nativeOnly) {
            RECT r{}; GetClientRect(parent,&r);
            clip(parent,WS_CLIPCHILDREN,true);
            if (::GetWindow(parent,GW_CHILD)!=m_hWnd || lastSize.cx!=r.right || lastSize.cy!=r.bottom)
                SetWindowPos(m_hWnd,HWND_TOP,0,0,r.right,r.bottom,SWP_NOACTIVATE);
            // NSIS 3.11 MUI abort headers are independent of the progress percentage.
            const auto header=textOf(GetDlgItem(parent,1037));
            const bool aborted=header==L"安装中止" || header==L"卸载中止" ||
                header==L"Installation Aborted" || header==L"Uninstallation Aborted";
            if (failed!=aborted) { failed=aborted; restyle=true; }
            for (auto& m : controls) m.seen=false;
            cutouts.clear();
            EnumChildWindows(parent,enumerate,reinterpret_cast<LPARAM>(this));
            for (auto& m : controls) if (!m.seen && m.control->IsVisible()) m.control->SetVisible(false);
            shape(r);
        }
        syncing=false; restyle=false;
    }
    LRESULT HandleMessage(UINT message, WPARAM wparam, LPARAM lparam) override {
        if (message == WM_CREATE) {
            paint.Init(m_hWnd);
            paint.SetNoActivate(true); // Native NSIS controls own OS focus, also on the initial paint.
            SetWindowSubclass(parent,hostProc,reinterpret_cast<UINT_PTR>(this),reinterpret_cast<DWORD_PTR>(this));
            root = new CContainerUI;
            root->SetBkColor(theme.bg);
            paint.AttachDialog(root);
            paint.AddNotifier(this);
            SetTimer(m_hWnd, syncTimer, 60, nullptr);
            return 0;
        }
        if (message==syncPage || (message == WM_TIMER && wparam == syncTimer)) { sync(); return 0; }
        if (message == WM_KEYDOWN && (wparam == VK_RETURN || wparam == VK_ESCAPE)) {
            if (wparam == VK_RETURN) {
                for (const auto& m : controls) if (m.seen && m.kind == L"Button" && m.type != BS_GROUPBOX &&
                    m.control == paint.GetFocus() && m.control->IsEnabled() && m.control->Activate()) return 0;
            }
            const int id = wparam == VK_ESCAPE ? IDCANCEL : IDOK;
            if (IsWindowEnabled(GetDlgItem(parent, id))) ::PostMessageW(parent, WM_COMMAND, id, 0);
            return 0;
        }
        if (message == WM_DESTROY) {
            RemoveWindowSubclass(parent,hostProc,reinterpret_cast<UINT_PTR>(this));
            restoreNative();
            KillTimer(m_hWnd, syncTimer);
        }
        LRESULT result{};
        if (paint.MessageHandler(message, wparam, lparam, result)) return result;
        return CWindowWnd::HandleMessage(message, wparam, lparam);
    }
};
std::unique_ptr<Surface> window;
}

NSIS_ENTRY(SetLanguage) {
    (void)parent;
    novaInstallerEnglish = Stack{stringSize, stackTop}.pop() == L"1033";
}

NSIS_ENTRY(Attach) {
    Stack stack{stringSize, stackTop};
    const auto title = stack.pop();
    if (!IsWindow(parent)) { stack.push(novaText(L"安装窗口尚未创建", L"Installer window is not available")); return; }
    if (!window) {
        CPaintManagerUI::SetInstance(module);
        window = std::make_unique<Surface>();
        window->parent = parent;
        if (!window->Create(parent, title.c_str(), WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0, 0, 0, 1, 1)) {
            window.reset(); stack.push(novaText(L"安装界面创建失败", L"Could not create the installer interface")); return;
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
