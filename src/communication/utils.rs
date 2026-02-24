/// Get a platform-independent temporary file path for the given identifier and extension
pub fn get_temp_path(identifier: &str, extension: &str) -> String {
    let temp_dir = std::env::temp_dir();
    format!("{}/{}.{}", temp_dir.display(), identifier, extension)
}
