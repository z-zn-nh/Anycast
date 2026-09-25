//! Windows 原生文件/文件夹选择对话框（IFileOpenDialog）。

use windows::core::Interface;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

/// 弹出系统对话框选择文件夹或文件；取消返回 None。
pub fn pick_path(folder: bool, title: &str) -> Option<String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut opts = dialog.GetOptions().ok()?;
        opts |= FOS_FORCEFILESYSTEM;
        if folder {
            opts |= FOS_PICKFOLDERS;
        }
        dialog.SetOptions(opts).ok()?;
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = dialog.SetTitle(windows::core::PCWSTR(wide.as_ptr()));
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = name.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as *const _));
        let _ = item.cast::<windows::core::IUnknown>();
        Some(s)
    }
}
