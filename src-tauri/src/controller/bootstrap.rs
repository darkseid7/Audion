//! Native build role and minimal controller startup. No desktop domain states.
use super::protocol::ApplicationMode;

#[tauri::command]
pub fn get_application_mode() -> ApplicationMode {
    native_application_mode()
}

pub fn native_application_mode() -> ApplicationMode {
    #[cfg(target_os = "android")]
    {
        ApplicationMode::Controller
    }
    #[cfg(not(target_os = "android"))]
    {
        ApplicationMode::Desktop
    }
}

#[cfg(mobile)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_os::init());
    #[cfg(target_os = "android")]
    let builder = builder
        .plugin(super::mobile::native_plugin())
        .invoke_handler(tauri::generate_handler![
            get_application_mode,
            super::mobile::controller_scan_pair,
            crate::commands::controller::controller_pair,
            crate::commands::controller::controller_request,
            crate::commands::controller::controller_connection,
            crate::commands::controller::controller_suspend,
            crate::commands::controller::controller_forget
        ]);
    #[cfg(not(target_os = "android"))]
    let builder = builder.invoke_handler(tauri::generate_handler![get_application_mode]);
    builder
        .run(tauri::generate_context!())
        .expect("error while running controller application");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_role_is_build_defined() {
        #[cfg(target_os = "android")]
        assert_eq!(native_application_mode(), ApplicationMode::Controller);
        #[cfg(not(target_os = "android"))]
        assert_eq!(native_application_mode(), ApplicationMode::Desktop);
    }
}
