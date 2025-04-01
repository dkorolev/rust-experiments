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
  for (link, dir) in [("src/lib", "../../../lib"), ("templates", "../../lib/templates")] {
    let link = Path::new(link);
    if !is_dir_or_symlink_to_dir(link) {
      symlink(dir, link).expect("Failed to create symlink");
    }
  }
}
