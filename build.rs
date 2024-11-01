#[cfg(windows)]
extern crate windows_exe_info;

#[cfg(windows)]
fn main() {
    windows_exe_info::icon::icon_ico(std::path::Path::new("icon/roomor.ico"));
}

#[cfg(not(windows))]
fn main() {}
