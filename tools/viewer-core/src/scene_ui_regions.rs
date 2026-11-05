//! Expose main-WebView UI through the embedded child's window region. No
//! copied framebuffer, renderer restart or parent reassignment. A noninteractive
//! black child supplies modal dimming above the existing native presentation.
use viewer_launch_contract::{SceneUiPointer, SceneUiRegion};

#[cfg(windows)]
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
struct Rect { left: i32, top: i32, right: i32, bottom: i32 }

#[cfg(windows)]
mod platform {
    use super::*;
    use anyhow::{bail, ensure, Result};
    #[repr(C)] struct Point { x: i32, y: i32 }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> isize;
        fn CombineRgn(dest: isize, source: isize, subtract: isize, mode: i32) -> i32;
        fn DeleteObject(object: isize) -> i32;
    }
    struct WindowUiState {
        // Keep unclipped rectangles: a later native resize may expose their tails.
        regions: Option<Vec<SceneUiRegion>>,
        backdrop: isize,
        refreshing: bool,
    }
    const STATE_PROP: &[u16] = &[78,111,118,97,46,83,99,101,110,101,85,105,0];
    const SUBCLASS_ID: usize = 0x4e565549;
    const STATIC_CLASS: &[u16] = &[83,84,65,84,73,67,0];
    type SubclassProc = unsafe extern "system" fn(isize,u32,usize,isize,usize,usize)->isize;
    #[link(name = "comctl32")]
    unsafe extern "system" {
        fn SetWindowSubclass(hwnd:isize,proc:SubclassProc,id:usize,data:usize)->i32;
        fn RemoveWindowSubclass(hwnd:isize,proc:SubclassProc,id:usize)->i32;
        fn DefSubclassProc(hwnd:isize,msg:u32,wp:usize,lp:isize)->isize;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn CreateWindowExW(ex:u32,class:*const u16,name:*const u16,style:u32,x:i32,y:i32,w:i32,h:i32,parent:isize,menu:isize,instance:isize,param:*mut std::ffi::c_void)->isize;
        fn DestroyWindow(hwnd:isize)->i32;
        fn SetPropW(hwnd:isize,name:*const u16,value:isize)->i32;
        fn GetPropW(hwnd:isize,name:*const u16)->isize;
        fn RemovePropW(hwnd:isize,name:*const u16)->isize;
        fn SetWindowPos(hwnd:isize,after:isize,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
        fn SetLayeredWindowAttributes(hwnd:isize,key:u32,alpha:u8,flags:u32)->i32;
        fn ShowWindow(hwnd:isize,show:i32)->i32;
        fn EnableWindow(hwnd: isize, enabled: i32) -> i32;
        fn IsWindowEnabled(hwnd:isize)->i32;
        fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
        fn SetWindowRgn(hwnd: isize, region: isize, redraw: i32) -> i32;
        fn GetParent(hwnd: isize) -> isize;
        fn InvalidateRect(hwnd: isize, rect: *const Rect, erase: i32) -> i32;
        fn SetFocus(hwnd: isize) -> isize;
        fn ReleaseCapture() -> i32;
        fn GetAsyncKeyState(key:i32)->i16;
        fn PostMessageW(hwnd: isize, message: u32, wparam: usize, lparam: isize) -> i32;
        fn ClientToScreen(hwnd: isize, point: *mut Point) -> i32;
    }
    struct Region(isize);
    impl Drop for Region { fn drop(&mut self) { if self.0 != 0 { unsafe { DeleteObject(self.0); } } } }
    // All state belongs to this HWND's event thread, never an unsafe Send/Sync owner.
    fn ensure_state(hwnd: isize) -> Result<*mut WindowUiState> {
        unsafe {
            let existing = GetPropW(hwnd, STATE_PROP.as_ptr()) as *mut WindowUiState;
            if !existing.is_null() { return Ok(existing); }
            let state = Box::into_raw(Box::new(WindowUiState { regions: None, backdrop: 0, refreshing: false }));
            if SetPropW(hwnd, STATE_PROP.as_ptr(), state as isize) == 0 {
                drop(Box::from_raw(state));
                anyhow::bail!("Scene UI state attachment: {}", std::io::Error::last_os_error());
            }
            if SetWindowSubclass(hwnd, ui_subclass, SUBCLASS_ID, 0) == 0 {
                RemovePropW(hwnd, STATE_PROP.as_ptr());
                drop(Box::from_raw(state));
                anyhow::bail!("Scene UI resize subscription failed");
            }
            Ok(state)
        }
    }
    unsafe extern "system" fn ui_subclass(hwnd:isize,msg:u32,wp:usize,lp:isize,_id:usize,_data:usize)->isize {
        unsafe {
            if msg == 0x0082 { // WM_NCDESTROY: child HWNDs are destroyed by USER32.
                let state = RemovePropW(hwnd, STATE_PROP.as_ptr()) as *mut WindowUiState;
                RemoveWindowSubclass(hwnd, ui_subclass, SUBCLASS_ID);
                if !state.is_null() { drop(Box::from_raw(state)); }
                return DefSubclassProc(hwnd,msg,wp,lp);
            }
            let result = DefSubclassProc(hwnd,msg,wp,lp);
            if msg == 0x0005 { // WM_SIZE: use the final client size, not stale UI dimensions.
                let _ = refresh(hwnd);
            }
            result
        }
    }
    fn resize_backdrop(hwnd:isize,child:isize) -> Result<()> {
        if child == 0 { return Ok(()); }
        unsafe {
            let mut rect = Rect { left:0,top:0,right:0,bottom:0 };
            ensure!(GetClientRect(hwnd,&mut rect)!=0,"Scene backdrop client bounds: {}",std::io::Error::last_os_error());
            ensure!(SetWindowPos(child,0,0,0,rect.right.max(0),rect.bottom.max(0),0x0010)!=0,
                "Scene backdrop resize: {}",std::io::Error::last_os_error()); // HWND_TOP, SWP_NOACTIVATE
        }
        Ok(())
    }
    fn refresh(hwnd:isize) -> Result<()> {
        unsafe {
            let state = GetPropW(hwnd,STATE_PROP.as_ptr()) as *mut WindowUiState;
            if state.is_null() || (*state).refreshing { return Ok(()); }
            (*state).refreshing = true;
            // No reference into the allocation survives a re-entrant USER32 call.
            let rects = (*state).regions.clone();
            let child = (*state).backdrop;
            let result = (|| {
                if let Some(rects) = rects { apply_region(hwnd,&rects)?; }
                resize_backdrop(hwnd,child)
            })();
            if GetPropW(hwnd,STATE_PROP.as_ptr()) == state as isize { (*state).refreshing=false; }
            result
        }
    }
    pub fn apply(hwnd: isize, rects: &[SceneUiRegion]) -> Result<()> {
        ensure!(rects.len() <= 64, "Too many scene UI regions");
        unsafe { (*ensure_state(hwnd)?).regions = Some(rects.to_vec()); }
        refresh(hwnd)
    }
    pub fn set_backdrop(hwnd:isize,opacity:f32) -> Result<()> {
        ensure!(opacity.is_finite() && (0.0..=1.0).contains(&opacity),"Invalid scene backdrop opacity");
        unsafe {
            let state = ensure_state(hwnd)?;
            let mut child = (*state).backdrop;
            if opacity == 0.0 {
                if child != 0 { ShowWindow(child,0); }
                return Ok(());
            }
            let alpha = (opacity * 255.0).round() as u8;
            if child == 0 {
                // Pure black STATIC. Layered + transparent + disabled never takes input,
                // NOACTIVATE never takes focus. This is a child, never a new top-level viewer.
                child = CreateWindowExW(0x00080000|0x00000020|0x08000000,
                    STATIC_CLASS.as_ptr(),std::ptr::null(),0x40000000|0x08000000|4,
                    0,0,0,0,hwnd,0,0,std::ptr::null_mut());
                ensure!(child!=0,"Scene backdrop creation: {}",std::io::Error::last_os_error());
                if SetLayeredWindowAttributes(child,0,alpha,2)==0 {
                    let error=std::io::Error::last_os_error();DestroyWindow(child);
                    return Err(error.into());
                }
                (*state).backdrop=child;
            } else {
                ensure!(SetLayeredWindowAttributes(child,0,alpha,2)!=0,"Scene backdrop alpha: {}",std::io::Error::last_os_error());
            }
            resize_backdrop(hwnd,child)?;
            ShowWindow(child,8); // SW_SHOWNA; the hidden test parent stays hidden.
        }
        Ok(())
    }
    fn apply_region(hwnd:isize,rects:&[SceneUiRegion]) -> Result<()> {
        unsafe {
            let mut bounds = Rect { left: 0, top: 0, right: 0, bottom: 0 };
            ensure!(GetClientRect(hwnd, &mut bounds) != 0, "Scene UI client bounds: {}", std::io::Error::last_os_error());
            let mut whole = Region(CreateRectRgn(0, 0, bounds.right, bounds.bottom));
            ensure!(whole.0 != 0, "Scene UI region allocation failed");
            for rect in rects {
                let left = rect.x.clamp(0, bounds.right.max(0));
                let top = rect.y.clamp(0, bounds.bottom.max(0));
                let right = (i64::from(rect.x) + i64::from(rect.width)).clamp(0, i64::from(bounds.right.max(0))) as i32;
                let bottom = (i64::from(rect.y) + i64::from(rect.height)).clamp(0, i64::from(bounds.bottom.max(0))) as i32;
                if right <= left || bottom <= top { continue; }
                let hole = Region(CreateRectRgn(left, top, right, bottom));
                ensure!(hole.0 != 0 && CombineRgn(whole.0, whole.0, hole.0, 4) != 0, "Scene UI region subtraction failed");
            }
            ensure!(SetWindowRgn(hwnd, whole.0, 1) != 0, "Scene UI region update: {}", std::io::Error::last_os_error());
            // USER32 owns the region only after successful SetWindowRgn.
            whole.0 = 0;
            let parent = GetParent(hwnd);
            if parent != 0 { InvalidateRect(parent, std::ptr::null(), 0); }
        }
        Ok(())
    }
    pub fn set_input_blocked(hwnd: isize, blocked: bool) -> Result<()> {
        unsafe {
            if blocked { ReleaseCapture(); }
            // Disabling input preserves the child surface and its GPU resources.
            EnableWindow(hwnd, i32::from(!blocked));
        }
        Ok(())
    }
    pub fn forward_pointer(hwnd: isize, event: &SceneUiPointer) -> Result<()> {
        // Posted messages bypass EnableWindow; a late WebView event must not
        // restore native focus/capture while a modal owns the input.
        if unsafe { IsWindowEnabled(hwnd) } == 0 { return Ok(()); }
        let (message, mut wparam) = match (event.kind.as_str(), event.button.unwrap_or(0)) {
            ("down", 0) => (0x0201, 1usize), ("up", 0) => (0x0202, 0),
            ("down", 1) => (0x0207, 0x10), ("up", 1) => (0x0208, 0),
            ("down", 2) => (0x0204, 2), ("up", 2) => (0x0205, 0),
            ("move", _) => (0x0200, 0), ("wheel", _) => (0x020a, 0),
            _ => bail!("Unsupported scene pointer event"),
        };
        let mut point = Point { x: event.x, y: event.y };
        unsafe {
            // Winit owns the matching press/release capture. Releasing before
            // its queued WM_*UP races an in-flight native drag.
            if event.kind == "down" { SetFocus(hwnd); }
            if GetAsyncKeyState(0x10)<0 {wparam|=4;} // MK_SHIFT
            if GetAsyncKeyState(0x11)<0 {wparam|=8;} // MK_CONTROL
            if event.kind == "wheel" {
                ensure!(ClientToScreen(hwnd, &mut point) != 0, "Scene pointer screen mapping failed");
                wparam |= (event.delta.unwrap_or(0).clamp(i16::MIN as i32, i16::MAX as i32) as u16 as usize) << 16;
            }
            let lparam = ((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize;
            if matches!(event.kind.as_str(),"down"|"up") {
                ensure!(PostMessageW(hwnd, 0x0200, wparam, lparam)!=0,"Scene pointer position dispatch: {}",std::io::Error::last_os_error());
            }
            ensure!(PostMessageW(hwnd, message, wparam, lparam) != 0, "Scene pointer dispatch: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)] pub use platform::{apply, forward_pointer, set_backdrop, set_input_blocked};
#[cfg(not(windows))]
pub fn apply(_hwnd: isize, regions: &[SceneUiRegion]) -> anyhow::Result<()> {
    anyhow::ensure!(regions.is_empty(), "Embedded scene UI regions require the Windows native host");
    Ok(())
}
#[cfg(not(windows))]
pub fn set_backdrop(_hwnd:isize,opacity:f32) -> anyhow::Result<()> {
    anyhow::ensure!(opacity == 0.0,"Native scene backdrop requires the Windows native host");
    Ok(())
}
#[cfg(not(windows))]
pub fn forward_pointer(_hwnd: isize, _event: &SceneUiPointer) -> anyhow::Result<()> {
    anyhow::bail!("Embedded scene pointer forwarding requires the Windows native host")
}

#[cfg(not(windows))]
pub fn set_input_blocked(_hwnd: isize, blocked: bool) -> anyhow::Result<()> {
    anyhow::ensure!(!blocked, "Native modal input ownership is not available on this platform");
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[repr(C)]
    struct Message { hwnd:isize,kind:u32,wp:usize,lp:isize,time:u32,point:[i32;2],private:u32 }
    #[link(name="user32")]
    unsafe extern "system" {
        fn CreateWindowExW(ex:u32,class:*const u16,name:*const u16,style:u32,x:i32,y:i32,w:i32,h:i32,parent:isize,menu:isize,instance:isize,param:*mut std::ffi::c_void)->isize;
        fn DestroyWindow(hwnd:isize)->i32;
        fn IsWindowEnabled(hwnd:isize)->i32;
        fn GetWindowLongW(hwnd:isize,index:i32)->i32;
        fn PeekMessageW(msg:*mut Message,hwnd:isize,min:u32,max:u32,remove:u32)->i32;
        fn GetParent(hwnd:isize)->isize;
        fn GetPropW(hwnd:isize,name:*const u16)->isize;
        fn GetClientRect(hwnd:isize,rect:*mut Rect)->i32;
        fn GetLayeredWindowAttributes(hwnd:isize,key:*mut u32,alpha:*mut u8,flags:*mut u32)->i32;
        fn GetWindow(hwnd:isize,command:u32)->isize;
        fn GetFocus()->isize;
        fn IsWindow(hwnd:isize)->i32;
        fn IsWindowVisible(hwnd:isize)->i32;
        fn SetWindowPos(hwnd:isize,after:isize,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
        fn GetWindowRgn(hwnd:isize,region:isize)->i32;
    }
    #[link(name="gdi32")]
    unsafe extern "system" {
        fn CreateRectRgn(l:i32,t:i32,r:i32,b:i32)->isize;
        fn PtInRegion(region:isize,x:i32,y:i32)->i32;
        fn DeleteObject(region:isize)->i32;
    }
    struct Window(isize);
    impl Drop for Window {fn drop(&mut self){unsafe{DestroyWindow(self.0);}}}
    #[test]
    fn actual_hidden_child_surface_clip_and_modal_input_preserve_window() {
        unsafe {
            let class:Vec<u16>="STATIC\0".encode_utf16().collect();
            let parent=Window(CreateWindowExW(0,class.as_ptr(),class.as_ptr(),0x80000000,0,0,640,480,0,0,0,std::ptr::null_mut()));
            assert_ne!(parent.0,0);
            let child=Window(CreateWindowExW(0,class.as_ptr(),class.as_ptr(),0x40000000,0,0,640,480,parent.0,0,0,std::ptr::null_mut()));
            assert_ne!(child.0,0);
            let before=GetWindowLongW(child.0,-16);
            assert_eq!(before&0x10000000,0,"the test never displays a desktop window");
            apply(child.0,&[SceneUiRegion{x:20,y:40,width:120,height:80}]).unwrap();
            let region=CreateRectRgn(0,0,0,0);assert_ne!(region,0);
            assert_ne!(GetWindowRgn(child.0,region),0);
            assert_eq!(PtInRegion(region,30,50),0);assert_ne!(PtInRegion(region,300,240),0);
            // The preserved source hole extends beyond the old client rect.
            // Enlarging must reveal the still-subtracted tail and new whole surface.
            apply(child.0,&[SceneUiRegion{x:600,y:40,width:180,height:80}]).unwrap();
            assert_ne!(SetWindowPos(child.0,0,0,0,800,600,0x0014),0);
            assert_ne!(GetWindowRgn(child.0,region),0);
            assert_eq!(PtInRegion(region,750,60),0);
            assert_ne!(PtInRegion(region,790,560),0);
            assert_ne!(SetWindowPos(child.0,0,0,0,100,80,0x0014),0);
            assert_ne!(SetWindowPos(child.0,0,0,0,800,600,0x0014),0);
            assert_ne!(GetWindowRgn(child.0,region),0);
            assert_eq!(PtInRegion(region,750,60),0,"resize retains original unclamped rectangles");
            let focus=GetFocus();
            set_backdrop(child.0,0.5).unwrap();
            let backdrop=GetWindow(child.0,5); // GW_CHILD
            assert_ne!(backdrop,0);assert_eq!(GetParent(backdrop),child.0);
            assert_eq!(GetWindowLongW(backdrop,-16)&0x40000000,0x40000000);
            assert_eq!(GetWindowLongW(backdrop,-20)&0x08080020,0x08080020);
            assert_eq!(IsWindowEnabled(backdrop),0);assert_eq!(GetFocus(),focus);
            assert_eq!(IsWindowVisible(parent.0),0);assert_eq!(IsWindowVisible(backdrop),0);
            let(mut key,mut alpha,mut flags)=(0,0,0);
            assert_ne!(GetLayeredWindowAttributes(backdrop,&mut key,&mut alpha,&mut flags),0);
            assert_eq!(alpha,128);assert_eq!(flags,2);
            // UI sends combined opacity 1-(1-a)*(1-b). Closing the top modal
            // restores the previous dimmer, then closes the last one without scene loss.
            set_backdrop(child.0,1.0-(1.0-0.5)*(1.0-0.4)).unwrap();
            assert_ne!(GetLayeredWindowAttributes(backdrop,&mut key,&mut alpha,&mut flags),0);
            assert_eq!(alpha,179);
            set_backdrop(child.0,0.5).unwrap();
            assert_ne!(GetLayeredWindowAttributes(backdrop,&mut key,&mut alpha,&mut flags),0);
            assert_eq!(alpha,128);
            assert_ne!(SetWindowPos(child.0,0,0,0,920,640,0x0014),0);
            let mut bounds=Rect{left:0,top:0,right:0,bottom:0};assert_ne!(GetClientRect(backdrop,&mut bounds),0);
            assert_eq!(bounds,Rect{left:0,top:0,right:920,bottom:640});
            set_backdrop(child.0,0.0).unwrap();
            assert_eq!(GetWindowLongW(backdrop,-16)&0x10000000,0);
            assert_ne!(SetWindowPos(child.0,0,0,0,940,660,0x0014),0);
            assert_eq!(GetWindowLongW(backdrop,-16)&0x10000000,0,"resize never re-shows a cleared backdrop");
            assert_eq!(GetFocus(),focus);
            set_input_blocked(child.0,true).unwrap();assert_eq!(IsWindowEnabled(child.0),0);
            assert_eq!(GetWindowLongW(child.0,-16)&0x10000000,before&0x10000000);
            let pointer=SceneUiPointer{kind:"down".into(),x:30,y:50,button:Some(0),delta:None};
            let mut message:Message=std::mem::zeroed();
            while PeekMessageW(&mut message,child.0,0x0200,0x020e,1)!=0 {}
            let blocked_focus=GetFocus();
            forward_pointer(child.0,&pointer).unwrap();
            assert_eq!(GetFocus(),blocked_focus,"late blocked input must not focus the native surface");
            assert_eq!(PeekMessageW(&mut message,child.0,0x0200,0x020e,1),0,"disabled child receives no posted mouse messages");
            set_input_blocked(child.0,false).unwrap();assert_ne!(IsWindowEnabled(child.0),0);
            forward_pointer(child.0,&pointer).unwrap();
            assert_ne!(PeekMessageW(&mut message,child.0,0x0200,0x020e,1),0);
            assert_eq!(message.kind,0x0200);
            assert_ne!(PeekMessageW(&mut message,child.0,0x0200,0x020e,1),0);
            assert_eq!(message.kind,0x0201,"ordinary enabled pointer forwarding remains active");
            apply(child.0,&[]).unwrap();assert_ne!(GetWindowRgn(child.0,region),0);assert_ne!(PtInRegion(region,30,50),0);
            DeleteObject(region);
            assert_eq!(GetWindowLongW(child.0,-16),before);
            let prop:Vec<u16>="Nova.SceneUi\0".encode_utf16().collect();
            assert_ne!(GetPropW(child.0,prop.as_ptr()),0);
            let child_hwnd=child.0;
            drop(child);
            assert_eq!(IsWindow(backdrop),0);
            assert_eq!(GetPropW(child_hwnd,prop.as_ptr()),0);
            assert_ne!(IsWindow(parent.0),0);
        }
    }
}
