use tempfile::TempPath;

pub fn build_temp_config(
    temp_db: &TempPath,
    temp_log: &TempPath,
    temp_prefix: &TempPath,
) -> String {
    let escaped = temp_db.as_os_str().to_str().unwrap().replace('\\', "\\\\");
    let trie_escaped = temp_log.as_os_str().to_str().unwrap().replace('\\', "\\\\");
    let prefix_escaped = temp_prefix
        .as_os_str()
        .to_str()
        .unwrap()
        .replace('\\', "\\\\");

    format!(
        r#"
    database = "{escaped}"
    defaultLog = "debug"
    caplog = {{ trieFile = "{trie_escaped}", dataPrefix = "{prefix_escaped}" }}"#
    )
}

pub fn get_binary_path(name: &str) -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("couldn't get current EXE path");
    let mut target_dir = exe.parent().unwrap();

    if target_dir.ends_with("deps") {
        target_dir = target_dir.parent().unwrap();
    }

    #[cfg(windows)]
    {
        return target_dir.join(format!("{}.exe", name).as_str());
    }

    #[cfg(not(windows))]
    {
        return target_dir.join(name);
    }
}
