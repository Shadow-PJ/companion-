//! Windows notifications (the ones that slide in at the bottom right and stay
//! in the notification center), for the few alerts you shouldn't miss even when
//! you're looking at another screen: a usage limit, Claude being back, a
//! message Glowby stopped to save your usage.
//!
//! Windows only shows notifications from apps it knows. A normal installer
//! registers that; Glowby is a single .exe, so it registers itself the same
//! way installers do: one registry key with its display name, under
//! HKEY_CURRENT_USER\Software\Classes\AppUserModelId\Shadow-PJ.Glowby
//! (your account only, removed again by "Uninstall" in the README).

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::HSTRING;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW,
};

pub const APP_ID: &str = "Shadow-PJ.Glowby";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Tells Windows the name (and icon) to show on Glowby's notifications.
fn register(icon: Option<&std::path::Path>) {
    let key_path = wide(&format!("Software\\Classes\\AppUserModelId\\{APP_ID}"));
    let mut key: HKEY = std::ptr::null_mut();
    unsafe {
        let made = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        );
        if made != 0 {
            return;
        }
        let set = |name: &str, value: &str| {
            let (name, value) = (wide(name), wide(value));
            RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32);
        };
        set("DisplayName", "Glowby");
        if let Some(icon) = icon {
            set("IconUri", &icon.to_string_lossy());
        }
        RegCloseKey(key);
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn show_now(title: &str, body: &str) -> windows::core::Result<()> {
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    )))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
}

/// Shows a Windows notification (in the background; failures are only logged).
pub fn show(icon: Option<std::path::PathBuf>, title: String, body: String) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        register(icon.as_deref());
        if let Err(e) = show_now(&title, &body) {
            crate::applog::line(format!("couldn't show a Windows notification: {e}"));
        }
    });
}
