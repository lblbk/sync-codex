#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn portable_webview_data_directory(
    executable: &std::path::Path,
    has_override: bool,
) -> Option<std::path::PathBuf> {
    if has_override {
        return None;
    }

    let directory = executable.parent()?.join("sync-codex-data");
    directory.is_dir().then_some(directory)
}

fn main() {
    #[cfg(target_os = "windows")]
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = portable_webview_data_directory(
            &executable,
            std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some(),
        ) {
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", directory);
        }
    }

    sync_codex_lib::run();
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::portable_webview_data_directory;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn portable_directory_is_opt_in_and_respects_existing_override() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let executable_path = std::env::current_exe().unwrap();
        let root = executable_path.parent().unwrap().join(format!(
            "sync-codex-portable-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let executable = root.join("sync-codex.exe");
        let data_directory = root.join("sync-codex-data");

        assert_eq!(portable_webview_data_directory(&executable, false), None);
        fs::create_dir(&data_directory).unwrap();
        assert_eq!(
            portable_webview_data_directory(&executable, false),
            Some(data_directory.clone())
        );
        assert_eq!(portable_webview_data_directory(&executable, true), None);

        fs::remove_dir(&data_directory).unwrap();
        fs::remove_dir(&root).unwrap();
    }
}
