//! Window handle type passed into enumeration (Win32 `HWND` on Windows; placeholder on other hosts).

#[cfg(windows)]
pub type UiaHwnd = windows::Win32::Foundation::HWND;

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UiaHwnd(pub isize);
