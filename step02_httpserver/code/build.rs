use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

fn is_dir_or_symlink_to_dir(path: &Path) -> bool {
  match fs::metadata(path) {
    Ok(meta) => meta.is_dir(),
    Err(_) => false,
  }
}
fn main() {
  let link = Path::new("lib");
  if !is_dir_or_symlink_to_dir(link) {
    symlink("../../lib", Path::new("lib")).expect("Failed to create symlink");
  }
}
