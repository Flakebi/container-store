use std::collections::HashSet;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;
use std::thread;

use failure::{bail, format_err};
use structopt::clap::AppSettings;
use structopt::StructOpt;

type Result<T> = std::result::Result<T, failure::Error>;

/// Create a nix store for containers with whitelisted files only.
#[derive(Clone, Debug, StructOpt)]
#[structopt(global_settings = &[AppSettings::ColoredHelp,
                                AppSettings::VersionlessSubcommands])]
struct Opt {
    /// The root directory for the overlays.
    #[structopt(long, default_value = "/var/lib/container-stores")]
    root: PathBuf,
    /// The name of the overlay that should be created.
    #[structopt(short, long)]
    name: String,
    #[structopt()]
    files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    let root = opt.root.join(&opt.name);

    // Create folders
    let merged_path = root.join("merged");
    let upper_path = root.join("upper");
    fs::create_dir_all(&merged_path)?;
    fs::create_dir_all(&upper_path)?;
    fs::create_dir_all(root.join("work"))?;

    if is_mounted(&merged_path)? {
        umount(&merged_path)?;
    }

    fs::set_permissions(&upper_path, fs::Permissions::from_mode(0o111))?;

    // Parallellize, brings down time from 1.1s to 0.65s (in debug mode)
    let file = opt.files.clone();
    let needed_paths = thread::spawn(move || get_needed_paths(&file));
    let upper_path2 = upper_path.clone();
    let current_removed = thread::spawn(move || get_paths(&upper_path2));

    let current_store = get_paths(Path::new("/nix/store"))?;

    let needed_paths = needed_paths.join().map_err(|_| format_err!("Failed to join thread"))??;
    let current_removed = current_removed.join().map_err(|_| format_err!("Failed to join thread"))??;

    // Make available by removing from upper dir
    let mut new_ctr = 0;
    for file in current_removed.intersection(&needed_paths) {
        fs::remove_file(upper_path.join(file))?;
        new_ctr += 1;
    }

    // Remove outdated paths from upper dir
    let mut outdated_ctr = 0;
    for file in current_removed.difference(&current_store) {
        // file is in current_removed but not in current_store
        fs::remove_file(upper_path.join(file))?;
        outdated_ctr += 1;
    }

    // Remove by adding to upper dir
    let current_available: HashSet<_> = current_store.difference(&current_removed).collect();
    let needed_paths_ref: HashSet<_> = needed_paths.iter().collect();
    let upper_path_str = upper_path.to_str().unwrap();
    let mut rm_ctr = 0;
    for file in current_available.difference(&needed_paths_ref) {
        // path, mode, device
        let c_file = CString::new(format!("{}/{}", upper_path_str, file))?;
        let res = unsafe { libc::mknod(c_file.as_ptr(), 0, 0) };
        if res != 0 {
            bail!("Failed to remove file {}", file);
        }
        rm_ctr += 1;
    }
    println!(
        "Made {} paths available, removed {} outdated and {} unneeded paths",
        new_ctr, outdated_ctr, rm_ctr
    );

    mount(&root)?;

    Ok(())
}

fn is_mounted(path: &Path) -> Result<bool> {
    let path_bytes = path.as_os_str().as_bytes();
    let output = Command::new("mount").output()?;
    if !output.status.success() {
        bail!("Failed to query mounts for {:?}", path);
    }
    Ok(output
        .stdout
        .windows(path_bytes.len())
        .any(|p| p == path_bytes))
}

fn umount(path: &Path) -> Result<()> {
    if !Command::new("umount").arg(path).status()?.success() {
        bail!("Failed to unmount {:?}", path);
    }
    Ok(())
}

fn mount(path: &Path) -> Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| format_err!("Failed to convert path to string"))?;
    if !Command::new("mount")
        .arg("-t")
        .arg("overlay")
        .arg("overlay")
        .arg("-o")
        .arg(format!("lowerdir={}/upper:/nix/store", path))
        .arg(format!("{}/merged", path))
        .status()?
        .success()
    {
        bail!("Failed to mount {}", path);
    }
    Ok(())
}

/// Get the recursive set of store dependencies.
fn get_needed_paths(src_paths: &[PathBuf]) -> Result<HashSet<String>> {
    let output = Command::new("nix-store")
        .arg("-qR")
        .args(src_paths)
        .output()?;
    if !output.status.success() {
        bail!("Failed to query nix store");
    }
    let output = str::from_utf8(&output.stdout)?;
    let len = "/nix/store/".len();
    Ok(output.lines().map(|l| l[len..].to_string()).collect())
}

/// Get the set of files in a directory.
fn get_paths(path: &Path) -> Result<HashSet<String>> {
    fs::read_dir(path)?
        .map(|entry| {
            entry.map_err(|e| e.into()).and_then(|e| {
                e.file_name()
                    .into_string()
                    .map_err(|s| format_err!("Failed to convert os string {:?}", s))
            })
        })
        .collect()
}
