//! Windows OLE 拖出:把 SFTP 侧栏条目拖到资源管理器/桌面。
//!
//! 实现方式:后台线程先下载到临时目录,主线程 DoDragDrop 提供
//! CF_HDROP(真实路径)。GetData 在下载未完成时阻塞等待(资源管理器
//! 显示"发现项目"属正常体验)。
//!
//! winapi 0.3.9 缺 IDropSource/DoDragDrop 声明,此处手写 vtable 与
//! extern 导入;IDataObject/FORMATETC/STGMEDIUM 用 winapi 现成的。
//! COM 对象布局:首字段为 vtable 指针(即 COM this 指针),其后是
//! 引用计数与共享状态,标准手写 COM 惯例。

#![allow(non_snake_case)]

use std::sync::{Arc, Condvar, Mutex};
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{BOOL, DWORD, UINT};
use winapi::shared::windef::POINT;
use winapi::shared::winerror::{
    DV_E_FORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, E_FAIL,
    E_OUTOFMEMORY, S_FALSE, S_OK,
};
use winapi::um::objidl::{
    IDataObject, IDataObjectVtbl, IEnumFORMATETC, FORMATETC, STGMEDIUM, TYMED_HGLOBAL,
};
use winapi::um::oleidl::DROPEFFECT_COPY;
use winapi::um::unknwnbase::{IUnknown, IUnknownVtbl};
use winapi::um::winbase::{GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use winapi::um::winuser::CF_HDROP;

type ULONG = u32;

const IID_IUNKNOWN: GUID = GUID {
    Data1: 0x00000000,
    Data2: 0x0000,
    Data3: 0x0000,
    Data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_IDATAOBJECT: GUID = GUID {
    Data1: 0x0000010E,
    Data2: 0x0000,
    Data3: 0x0000,
    Data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

// winapi 未声明的 IDropSource / DoDragDrop ---------------------------------

#[repr(C)]
pub struct IDropSourceVtbl {
    pub parent: IUnknownVtbl,
    pub QueryContinueDrag: unsafe extern "system" fn(
        This: *mut IDropSource,
        fEscapePressed: BOOL,
        grfKeyState: DWORD,
    ) -> HRESULT,
    pub GiveFeedback: unsafe extern "system" fn(This: *mut IDropSource, dwEffect: DWORD) -> HRESULT,
}

#[repr(C)]
pub struct IDropSource {
    pub lpVtbl: *const IDropSourceVtbl,
}

use winapi::shared::ntdef::HRESULT;

#[link(name = "ole32")]
extern "system" {
    pub fn DoDragDrop(
        pDataObj: *mut IDataObject,
        pDropSource: *mut IDropSource,
        dwOKEffects: DWORD,
        pdwEffect: *mut DWORD,
    ) -> HRESULT;
}

/// CF_HDROP 的 DROPFILES 头(20 字节,后接宽字符路径列表,双 NUL 结尾)
#[repr(C)]
struct DROPFILES {
    pFiles: UINT,
    pt: POINT,
    fNC: BOOL,
    fWide: BOOL,
}

const MK_LBUTTON: DWORD = 1;

// IDataObject 对象 ----------------------------------------------------------

/// COM this 指针即指向本结构首字段(vtbl),标准手写 COM 布局;
/// repr(C) 保证 vtbl 位于偏移 0
#[repr(C)]
struct DataObjectImpl {
    vtbl: &'static IDataObjectVtbl,
    refs: AtomicUsize,
    shared: Arc<DragOutShared>,
}

use std::sync::atomic::{AtomicUsize, Ordering};

/// 拖出共享状态:后台下载线程写,COM GetData 读
struct DragOutShared {
    ready: Mutex<Vec<std::path::PathBuf>>,
    cv: Condvar,
    error: Mutex<Option<String>>,
    cancelled: Mutex<bool>,
}

impl DragOutShared {
    /// 阻塞等待下载完成;返回 None = 失败或已取消
    fn wait_ready(&self) -> Option<Vec<std::path::PathBuf>> {
        let mut ready = self.ready.lock().unwrap();
        loop {
            if !ready.is_empty() {
                return Some(ready.clone());
            }
            if let Some(err) = self.error.lock().unwrap().as_ref() {
                log::error!("sftp dragout: {err}");
                return None;
            }
            if *self.cancelled.lock().unwrap() {
                return None;
            }
            ready = self.cv.wait(ready).unwrap();
        }
    }
}

unsafe fn obj(this: *mut IDataObject) -> *mut DataObjectImpl {
    this as *mut DataObjectImpl
}

// IUnknown ------------------------------------------------------------

unsafe extern "system" fn do_query_interface(
    this: *mut IUnknown,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    let iid = &*riid;
    let eq = |a: &GUID, b: &GUID| {
        a.Data1 == b.Data1
            && a.Data2 == b.Data2
            && a.Data3 == b.Data3
            && a.Data4 == b.Data4
    };
    if eq(iid, &IID_IUNKNOWN) || eq(iid, &IID_IDATAOBJECT) {
        // 同一对象同时就是 IDataObject
        *ppv = this as *mut std::ffi::c_void;
        ((*obj(this as *mut IDataObject)).refs).fetch_add(1, Ordering::Relaxed);
        S_OK
    } else {
        *ppv = std::ptr::null_mut();
        E_FAIL
    }
}

unsafe extern "system" fn do_add_ref(this: *mut IUnknown) -> ULONG {
    ((*obj(this as *mut IDataObject)).refs).fetch_add(1, Ordering::Relaxed) as ULONG + 1
}

unsafe extern "system" fn do_release(this: *mut IUnknown) -> ULONG {
    let o = obj(this as *mut IDataObject);
    let prev = (*o).refs.fetch_sub(1, Ordering::Relaxed);
    if prev == 1 {
        drop(Box::from_raw(o));
        0
    } else {
        (prev - 1) as ULONG
    }
}

// IDataObject ---------------------------------------------------------

unsafe extern "system" fn do_get_data(
    this: *mut IDataObject,
    pformatetc: *const FORMATETC,
    pmedium: *mut STGMEDIUM,
) -> HRESULT {
    let fmt = &*pformatetc;
    if fmt.cfFormat != CF_HDROP as u16 || fmt.tymed != TYMED_HGLOBAL as u32 {
        return DV_E_FORMATETC;
    }
    let paths = match (*obj(this)).shared.wait_ready() {
        Some(p) => p,
        None => return E_FAIL,
    };

    let mut wide: Vec<u16> = vec![];
    for p in &paths {
        wide.extend(p.as_os_str().to_string_lossy().encode_utf16());
        wide.push(0);
    }
    wide.push(0);
    let head = std::mem::size_of::<DROPFILES>();
    let total = head + wide.len() * 2;

    let hglobal = GlobalAlloc(GMEM_MOVEABLE, total);
    if hglobal.is_null() {
        return E_OUTOFMEMORY;
    }
    let ptr = GlobalLock(hglobal) as *mut u8;
    if ptr.is_null() {
        GlobalFree(hglobal);
        return E_OUTOFMEMORY;
    }
    std::ptr::write(
        ptr as *mut DROPFILES,
        DROPFILES {
            pFiles: head as UINT,
            pt: POINT { x: 0, y: 0 },
            fNC: 0,
            fWide: 1,
        },
    );
    let dst = ptr.add(head) as *mut u16;
    std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
    GlobalUnlock(hglobal);

    (*pmedium).tymed = TYMED_HGLOBAL;
    // winapi 以 `u: *mut STGMEDIUM_u` 表达内嵌 union;C 中 union 与
    // handle 同址,直接把 HGLOBAL 放进 u 指针位即为 TYMED_HGLOBAL 布局
    (*pmedium).u = hglobal as *mut winapi::um::objidl::STGMEDIUM_u;
    (*pmedium).pUnkForRelease = std::ptr::null_mut();
    S_OK
}

unsafe extern "system" fn do_get_data_here(
    _this: *mut IDataObject,
    _pformatetc: *const FORMATETC,
    _pmedium: *mut STGMEDIUM,
) -> HRESULT {
    E_FAIL
}

unsafe extern "system" fn do_query_get_data(
    _this: *mut IDataObject,
    pformatetc: *const FORMATETC,
) -> HRESULT {
    let fmt = &*pformatetc;
    if fmt.cfFormat == CF_HDROP as u16 && fmt.tymed == TYMED_HGLOBAL as u32 {
        S_OK
    } else {
        DV_E_FORMATETC
    }
}

unsafe extern "system" fn do_get_canonical_format_etc(
    _this: *mut IDataObject,
    _pformatetc_in: *const FORMATETC,
    pformatetc_out: *mut FORMATETC,
) -> HRESULT {
    (*pformatetc_out).ptd = std::ptr::null_mut();
    S_FALSE
}

unsafe extern "system" fn do_set_data(
    _this: *mut IDataObject,
    _pformatetc: *const FORMATETC,
    _pformatetc_out: *const FORMATETC,
    _f_release: BOOL,
) -> HRESULT {
    E_FAIL
}

unsafe extern "system" fn do_enum_format_etc(
    _this: *mut IDataObject,
    _direction: DWORD,
    ppenum: *mut *mut IEnumFORMATETC,
) -> HRESULT {
    *ppenum = std::ptr::null_mut();
    E_FAIL
}

unsafe extern "system" fn do_d_advise(
    _this: *mut IDataObject,
    _pformatetc: *const FORMATETC,
    _advf: DWORD,
    _sink: *const winapi::um::objidl::IAdviseSink,
    _connection: *mut DWORD,
) -> HRESULT {
    E_FAIL
}

unsafe extern "system" fn do_d_unadvise(_this: *mut IDataObject, _connection: DWORD) -> HRESULT {
    E_FAIL
}

unsafe extern "system" fn do_enum_d_advise(
    _this: *mut IDataObject,
    _enum: *const *const winapi::um::objidl::IEnumSTATDATA,
) -> HRESULT {
    E_FAIL
}

// IDropSource(无状态单例,vtable 绑全局) -----------------------------------

unsafe extern "system" fn ds_query_interface(
    _this: *mut winapi::um::unknwnbase::IUnknown,
    _riid: *const GUID,
    _ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    E_FAIL
}

unsafe extern "system" fn ds_add_ref(_this: *mut winapi::um::unknwnbase::IUnknown) -> ULONG {
    1
}

unsafe extern "system" fn ds_release(_this: *mut winapi::um::unknwnbase::IUnknown) -> ULONG {
    1
}

unsafe extern "system" fn ds_query_continue_drag(
    _this: *mut IDropSource,
    f_escape_pressed: BOOL,
    grf_key_state: DWORD,
) -> HRESULT {
    if f_escape_pressed != 0 {
        DRAGDROP_S_CANCEL
    } else if grf_key_state & MK_LBUTTON == 0 {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}

unsafe extern "system" fn ds_give_feedback(_this: *mut IDropSource, _effect: DWORD) -> HRESULT {
    DRAGDROP_S_USEDEFAULTCURSORS
}

// 静态 vtable -----------------------------------------------------------

static DATA_OBJECT_VTBL: IDataObjectVtbl = {
    IDataObjectVtbl {
        parent: IUnknownVtbl {
            QueryInterface: do_query_interface,
            AddRef: do_add_ref,
            Release: do_release,
        },
        GetData: do_get_data,
        GetDataHere: do_get_data_here,
        QueryGetData: do_query_get_data,
        GetCanonicalFormatEtc: do_get_canonical_format_etc,
        SetData: do_set_data,
        EnumFormatEtc: do_enum_format_etc,
        DAdvise: do_d_advise,
        DUnadvise: do_d_unadvise,
        EnumDAdvise: do_enum_d_advise,
    }
};

static DROP_SOURCE_VTBL: IDropSourceVtbl = IDropSourceVtbl {
    parent: IUnknownVtbl {
        QueryInterface: ds_query_interface,
        AddRef: ds_add_ref,
        Release: ds_release,
    },
    QueryContinueDrag: ds_query_continue_drag,
    GiveFeedback: ds_give_feedback,
};

// vtable 只含函数指针,跨线程共享安全
unsafe impl Sync for IDropSource {}
unsafe impl Send for IDropSource {}

static DROP_SOURCE: IDropSource = IDropSource {
    lpVtbl: &DROP_SOURCE_VTBL,
};

/// 启动一次拖出:spawn 后台下载,然后进入 DoDragDrop 模态循环。
/// 返回最终效果(DROPEFFECT_COPY 位)。
///
/// 调用前提:主线程已 OleInitialize(失败可容忍,拖出会静默失败)。
pub unsafe fn sftp_start_drag_out(
    downloads: Vec<(wezterm_ssh::Sftp, wezterm_ssh::Utf8PathBuf, bool)>,
    temp_dir: std::path::PathBuf,
) -> anyhow::Result<DWORD> {
    let shared = Arc::new(DragOutShared {
        ready: Mutex::new(vec![]),
        cv: Condvar::new(),
        error: Mutex::new(None),
        cancelled: Mutex::new(false),
    });

    let dl_shared = Arc::clone(&shared);
    std::thread::spawn(move || {
        let mut all = vec![];
        for (sftp, remote, is_dir) in downloads {
            match crate::termwindow::sftp_transfer::sftp_download_to(
                &sftp,
                &remote,
                is_dir,
                &temp_dir,
                &|_, _| {},
            ) {
                Ok(p) => all.push(p),
                Err(err) => {
                    *dl_shared.error.lock().unwrap() = Some(format!("{err:#}"));
                    dl_shared.cv.notify_all();
                    return;
                }
            }
        }
        *dl_shared.ready.lock().unwrap() = all;
        dl_shared.cv.notify_all();
    });

    let data_object = Box::into_raw(Box::new(DataObjectImpl {
        vtbl: &DATA_OBJECT_VTBL,
        refs: AtomicUsize::new(1),
        shared: Arc::clone(&shared),
    })) as *mut IDataObject;

    let mut effect: DWORD = 0;
    let hr = DoDragDrop(data_object, &DROP_SOURCE as *const _ as *mut _, DROPEFFECT_COPY, &mut effect);

    if hr != DRAGDROP_S_DROP {
        // 取消:让等待中的 GetData 尽快返回
        *shared.cancelled.lock().unwrap() = true;
        shared.cv.notify_all();
    }
    // COM 侧若仍持有引用,Release 会兜底回收;模态循环结束后
    // 系统一定已 Release,这里主动再放一次我们自己的初始引用
    if hr == DRAGDROP_S_DROP || hr == DRAGDROP_S_CANCEL {
        // 系统已释放其引用;释放我们初始的这一个
        ((*data_object).lpVtbl as *const IDataObjectVtbl)
            .as_ref()
            .unwrap();
        do_release(data_object as *mut IUnknown);
    }

    Ok(effect)
}
