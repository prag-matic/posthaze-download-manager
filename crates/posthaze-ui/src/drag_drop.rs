//! Outbound OS drag-and-drop support for completed downloads.
//!
//! GPUI supports inbound OS file drops (`ExternalPaths`) and in-window drags,
//! but does not expose an outbound OS-level file drag API. On Windows, we
//! initiate a native OLE `DoDragDrop` session carrying `CF_HDROP` (`DROPFILES`).
//!
//! Because `DoDragDrop` runs a nested modal Win32 message loop that dispatches
//! `WM_PAINT` / `WM_TIMER` to GPUI's `WndProc`, calling it synchronously inside a
//! GPUI event listener would re-enter GPUI while `App` / `Window` `RefCell`s
//! are borrowed and panic. Instead, GPUI posts `WM_POSTHAZE_START_DRAG` to a
//! lightweight `SetWindowSubclass` handler on the main `HWND`, returning from
//! the GPUI handler first so `DoDragDrop` executes at the top level of the
//! Win32 message loop with zero GPUI borrows held.

use std::path::PathBuf;

use gpui_kit::{Pixels, Point};

/// Minimum pointer travel (in logical pixels) before a left-button press on a
/// completed row transitions from a click into an OS file drag.
pub(crate) const FILE_DRAG_THRESHOLD_PX: f32 = 6.0;

/// Returns `true` when pointer travel from `start` to `current` exceeds
/// [`FILE_DRAG_THRESHOLD_PX`].
pub(crate) fn exceeds_drag_threshold(start: Point<Pixels>, current: Point<Pixels>) -> bool {
    let dx = f32::from(current.x - start.x);
    let dy = f32::from(current.y - start.y);
    (dx * dx + dy * dy) >= (FILE_DRAG_THRESHOLD_PX * FILE_DRAG_THRESHOLD_PX)
}

/// Installs the Win32 subclass hook used to dispatch `DoDragDrop` outside
/// GPUI's `RefCell` borrows. Safe to call multiple times for the same window.
pub(crate) fn install_drag_subclass(window: &gpui_kit::Window) {
    #[cfg(target_os = "windows")]
    win32::install_drag_subclass(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

/// Schedules an outbound OS file drag for `paths` on `window`.
pub(crate) fn request_file_drag(window: &gpui_kit::Window, paths: Vec<PathBuf>) {
    #[cfg(target_os = "windows")]
    win32::request_file_drag(window, paths);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, paths);
}

#[cfg(target_os = "windows")]
mod win32 {
    use std::collections::{HashMap, HashSet};
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC,
        DV_E_TYMED, E_INVALIDARG, E_NOTIMPL, FALSE, GlobalFree, HGLOBAL, HWND, LPARAM, LRESULT,
        POINT, S_OK, TRUE, WPARAM,
    };
    use windows::Win32::System::Com::{
        DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
        IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{
        GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows::Win32::System::Ole::{
        CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DoDragDrop, IDropSource,
        IDropSource_Impl, OleInitialize, OleUninitialize,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows::Win32::UI::Shell::{
        DROPFILES, DefSubclassProc, RemoveWindowSubclass, SHCreateStdEnumFmtEtc, SetWindowSubclass,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP, WM_NCDESTROY};
    use windows_core::{BOOL, Ref, implement};

    /// Custom `WM_APP` message dispatched outside GPUI borrows to start `DoDragDrop`.
    const WM_POSTHAZE_START_DRAG: u32 = WM_APP + 0x41;
    /// Unique Win32 subclass identifier (`"PHDR"`).
    const DRAG_SUBCLASS_ID: usize = 0x5048_4452;

    fn pending_drag_map() -> &'static Mutex<HashMap<isize, Vec<PathBuf>>> {
        static MAP: OnceLock<Mutex<HashMap<isize, Vec<PathBuf>>>> = OnceLock::new();
        MAP.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn installed_hwnds() -> &'static Mutex<HashSet<isize>> {
        static SET: OnceLock<Mutex<HashSet<isize>>> = OnceLock::new();
        SET.get_or_init(|| Mutex::new(HashSet::new()))
    }

    fn win32_hwnd(window: &gpui_kit::Window) -> Option<HWND> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut c_void)),
            _ => None,
        }
    }

    pub(super) fn install_drag_subclass(window: &gpui_kit::Window) {
        let Some(hwnd) = win32_hwnd(window) else {
            return;
        };
        let key = hwnd.0 as isize;
        let Ok(mut set) = installed_hwnds().lock() else {
            return;
        };
        if set.contains(&key) {
            return;
        }
        let ok = unsafe { SetWindowSubclass(hwnd, Some(drag_subclass_proc), DRAG_SUBCLASS_ID, 0) };
        if ok.as_bool() {
            set.insert(key);
        }
    }

    pub(super) fn request_file_drag(window: &gpui_kit::Window, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let Some(hwnd) = win32_hwnd(window) else {
            return;
        };
        install_drag_subclass(window);
        if let Ok(mut map) = pending_drag_map().lock() {
            map.insert(hwnd.0 as isize, paths);
        } else {
            return;
        }
        let _ = unsafe { PostMessageW(Some(hwnd), WM_POSTHAZE_START_DRAG, WPARAM(0), LPARAM(0)) };
    }

    unsafe extern "system" fn drag_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        uid_subclass: usize,
        _ref_data: usize,
    ) -> LRESULT {
        if msg == WM_POSTHAZE_START_DRAG {
            let paths = pending_drag_map()
                .lock()
                .ok()
                .and_then(|mut map| map.remove(&(hwnd.0 as isize)));
            if let Some(paths) = paths
                && !paths.is_empty()
            {
                perform_ole_file_drag(&paths);
            }
            return LRESULT(0);
        }
        if msg == WM_NCDESTROY {
            let key = hwnd.0 as isize;
            if let Ok(mut set) = installed_hwnds().lock() {
                set.remove(&key);
            }
            if let Ok(mut map) = pending_drag_map().lock() {
                map.remove(&key);
            }
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(drag_subclass_proc), uid_subclass) };
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    /// Builds the raw `DROPFILES` + double-null-terminated UTF-16 path buffer
    /// expected by `CF_HDROP`.
    pub(super) fn build_dropfiles_bytes(paths: &[PathBuf]) -> Vec<u8> {
        let header_size = std::mem::size_of::<DROPFILES>();
        let header = DROPFILES {
            pFiles: header_size as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: FALSE,
            fWide: TRUE,
        };
        let mut wide_units: Vec<u16> = Vec::new();
        for path in paths {
            wide_units.extend(path.as_os_str().encode_wide());
            wide_units.push(0);
        }
        wide_units.push(0);

        let mut buf = Vec::with_capacity(header_size + wide_units.len() * 2);
        let header_bytes = unsafe {
            std::slice::from_raw_parts((&header as *const DROPFILES).cast::<u8>(), header_size)
        };
        buf.extend_from_slice(header_bytes);
        for unit in wide_units {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        buf
    }

    fn build_dropfiles_hglobal(paths: &[PathBuf]) -> windows_core::Result<HGLOBAL> {
        let bytes = build_dropfiles_bytes(paths);
        let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()) }?;
        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            let _ = unsafe { GlobalFree(Some(hglobal)) };
            return Err(windows_core::Error::from_thread());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(hglobal);
        }
        Ok(hglobal)
    }

    fn hdrop_format() -> FORMATETC {
        FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn matches_hdrop(fmt: *const FORMATETC) -> Result<(), windows_core::HRESULT> {
        let Some(fmt) = (unsafe { fmt.as_ref() }) else {
            return Err(E_INVALIDARG);
        };
        if fmt.cfFormat != CF_HDROP.0 {
            return Err(DV_E_FORMATETC);
        }
        if fmt.dwAspect != DVASPECT_CONTENT.0 {
            return Err(DV_E_FORMATETC);
        }
        if (fmt.tymed & (TYMED_HGLOBAL.0 as u32)) == 0 {
            return Err(DV_E_TYMED);
        }
        Ok(())
    }

    #[implement(IDataObject)]
    struct FileDataObject {
        paths: Vec<PathBuf>,
    }

    impl IDataObject_Impl for FileDataObject_Impl {
        fn GetData(&self, pformatetcin: *const FORMATETC) -> windows_core::Result<STGMEDIUM> {
            if let Err(hr) = matches_hdrop(pformatetcin) {
                return Err(windows_core::Error::from_hresult(hr));
            }
            let hglobal = build_dropfiles_hglobal(&self.paths)?;
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: hglobal },
                pUnkForRelease: std::mem::ManuallyDrop::new(None),
            })
        }

        fn GetDataHere(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *mut STGMEDIUM,
        ) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> windows_core::HRESULT {
            match matches_hdrop(pformatetc) {
                Ok(()) => S_OK,
                Err(hr) => hr,
            }
        }

        fn GetCanonicalFormatEtc(
            &self,
            _pformatectin: *const FORMATETC,
            pformatetcout: *mut FORMATETC,
        ) -> windows_core::HRESULT {
            if let Some(out) = unsafe { pformatetcout.as_mut() } {
                out.ptd = std::ptr::null_mut();
            }
            E_NOTIMPL
        }

        fn SetData(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *const STGMEDIUM,
            _frelease: BOOL,
        ) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn EnumFormatEtc(&self, dwdirection: u32) -> windows_core::Result<IEnumFORMATETC> {
            if dwdirection != DATADIR_GET.0 as u32 {
                return Err(windows_core::Error::from_hresult(E_NOTIMPL));
            }
            let formats = [hdrop_format()];
            unsafe { SHCreateStdEnumFmtEtc(&formats) }
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: Ref<'_, IAdviseSink>,
        ) -> windows_core::Result<u32> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn DUnadvise(&self, _dwconnection: u32) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn EnumDAdvise(&self) -> windows_core::Result<IEnumSTATDATA> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }
    }

    #[implement(IDropSource)]
    struct FileDropSource;

    impl IDropSource_Impl for FileDropSource_Impl {
        fn QueryContinueDrag(
            &self,
            fescapepressed: BOOL,
            grfkeystate: MODIFIERKEYS_FLAGS,
        ) -> windows_core::HRESULT {
            if fescapepressed.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            if !grfkeystate.contains(MK_LBUTTON) {
                return DRAGDROP_S_DROP;
            }
            S_OK
        }

        fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> windows_core::HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    fn perform_ole_file_drag(paths: &[PathBuf]) {
        let _ = unsafe { ReleaseCapture() };

        let should_uninit = unsafe { OleInitialize(None) }.is_ok();

        let data_object: IDataObject = FileDataObject {
            paths: paths.to_vec(),
        }
        .into();
        let drop_source: IDropSource = FileDropSource.into();
        let mut effect = DROPEFFECT(0);

        let _ = unsafe {
            DoDragDrop(
                &data_object,
                &drop_source,
                DROPEFFECT_COPY | DROPEFFECT_LINK,
                &mut effect,
            )
        };

        if should_uninit {
            unsafe { OleUninitialize() };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn build_dropfiles_bytes_encodes_header_and_double_null_utf16_paths() {
            let paths = vec![
                PathBuf::from(r"C:\Downloads\alpha.zip"),
                PathBuf::from(r"C:\Downloads\beta.iso"),
            ];
            let bytes = build_dropfiles_bytes(&paths);
            let header_size = std::mem::size_of::<DROPFILES>();
            assert!(bytes.len() > header_size + 4);

            let header = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<DROPFILES>()) };
            let p_files = header.pFiles;
            let f_wide = header.fWide;
            assert_eq!(p_files as usize, header_size);
            assert_eq!(f_wide, TRUE);

            let wide: Vec<u16> = bytes[header_size..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();

            let mut expected: Vec<u16> = Vec::new();
            expected.extend(paths[0].as_os_str().encode_wide());
            expected.push(0);
            expected.extend(paths[1].as_os_str().encode_wide());
            expected.push(0);
            expected.push(0);
            assert_eq!(wide, expected);
        }

        #[test]
        fn file_data_object_accepts_hdrop_and_rejects_other_formats() {
            let obj: IDataObject = FileDataObject {
                paths: vec![PathBuf::from(r"C:\Downloads\sample.bin")],
            }
            .into();

            let valid = hdrop_format();
            assert_eq!(unsafe { obj.QueryGetData(&valid) }, S_OK);

            let mut wrong_cf = valid;
            wrong_cf.cfFormat = 1; // CF_TEXT
            assert_eq!(unsafe { obj.QueryGetData(&wrong_cf) }, DV_E_FORMATETC);

            let medium = unsafe { obj.GetData(&valid) }.expect("CF_HDROP GetData should succeed");
            assert_eq!(medium.tymed, TYMED_HGLOBAL.0 as u32);
            let hglobal = unsafe { medium.u.hGlobal };
            let _ = unsafe { GlobalFree(Some(hglobal)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Point, px};

    use super::{FILE_DRAG_THRESHOLD_PX, exceeds_drag_threshold};

    #[test]
    fn exceeds_drag_threshold_respects_radius() {
        let origin = Point::new(px(100.0), px(100.0));
        assert!(!exceeds_drag_threshold(origin, Point::new(px(103.0), px(103.0))));
        assert!(exceeds_drag_threshold(
            origin,
            Point::new(px(100.0 + FILE_DRAG_THRESHOLD_PX), px(100.0))
        ));
    }
}
