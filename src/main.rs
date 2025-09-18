use anyhow::{bail, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

const GO_DL_URL: &str = "https://go.dev/dl/";
const GO_API_URL: &str = "https://go.dev/dl/?mode=json";
const INSTALL_DIR: &str = "/usr/local";

// Structs to deserialize the JSON response from the Go API.
#[derive(Deserialize, Debug)]
struct GoRelease {
    files: Vec<GoFile>,
}

#[derive(Deserialize, Debug)]
struct GoFile {
    filename: String,
    os: String,
    arch: String,
    version: String,
    sha256: String,
    size: u64,
    kind: String,
}

fn main() -> Result<()> {
    println!("--- Go Installer ---");
    if env::var("SUDO_USER").is_err() {
        bail!("This must be run with sudo to install Go in '{}'.", INSTALL_DIR);
    }

    // 1. Detect Architecture and Fetch Release Info from API
    let os_arch = match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        unsupported => bail!("Unsupported architecture: {}", unsupported),
    };
    println!("✔ Detected Architecture: {}", os_arch);

    let release_info = get_latest_go_release(os_arch)?;
    println!("✔ Found Latest Go Version: {}", release_info.version);

    // 2. Download Tarball
    let download_url = format!("{}{}", GO_DL_URL, release_info.filename);
    let tarball_path = env::temp_dir().join(&release_info.filename);
    download_file(&download_url, &tarball_path, release_info.size)?;

    // 3. Verify Checksum (using API data)
    verify_checksum(&release_info.sha256, &tarball_path)?;
    println!("✔ Checksum Verified");

    // 4. Install
    install_go(&tarball_path)?;
    println!("✔ Go Installed to {}/go", INSTALL_DIR);

    // 5. Final User Instruction
    println!("\n--- ACTION REQUIRED ---");
    println!("Go is installed. To complete setup, add Go to your PATH.");
    println!("Run this command or add it to your shell profile (~/.profile, ~/.bashrc, etc.):");
    println!("\n  echo 'export PATH=$PATH:{}/go/bin' >> ~/.profile && source ~/.profile\n", INSTALL_DIR);

    fs::remove_file(&tarball_path)?;
    Ok(())
}

// Fetches release data and finds the latest stable archive for the given architecture.
fn get_latest_go_release(arch: &str) -> Result<GoFile> {
    let releases: Vec<GoRelease> = ureq::get(GO_API_URL).call()?.into_json()?;

    // Find the latest stable release for Linux archives.
    for release in releases {
        if let Some(file) = release.files.into_iter().find(|f| {
            f.os == "linux" && f.arch == arch && f.kind == "archive"
        }) {
            return Ok(file); // Return the first one found (latest version)
        }
    }
    bail!("Could not find a stable Go release for linux-{}", arch)
}

// Downloads a file with a progress bar.
fn download_file(url: &str, path: &Path, total_size: u64) -> Result<()> {
    let res = ureq::get(url).call()?;
    let pb = ProgressBar::new(total_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})")?
        .progress_chars("=>-"));
    pb.set_message(format!("Downloading {}", path.file_name().unwrap().to_str().unwrap()));

    let mut file = File::create(path)?;
    io::copy(&mut pb.wrap_read(res.into_reader()), &mut file)?;

    pb.finish_with_message("Download complete.");
    Ok(())
}

// Verifies the SHA256 checksum using the expected hash from the API.
fn verify_checksum(expected_checksum: &str, file_path: &Path) -> Result<()> {
    let mut file = File::open(file_path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let calculated_checksum = format!("{:x}", hasher.finalize());

    if calculated_checksum != expected_checksum {
        bail!(
            "Checksum mismatch!\n  Expected:   {}\n  Calculated: {}",
            expected_checksum, calculated_checksum
        );
    }
    Ok(())
}

// Removes any old installation and extracts the new one.
fn install_go(tarball_path: &Path) -> Result<()> {
    let go_path = PathBuf::from(INSTALL_DIR).join("go");
    if go_path.exists() {
        println!("- Removing existing Go installation...");
        fs::remove_dir_all(&go_path)?;
    }
    println!("- Extracting Go archive...");
    let tar_gz = File::open(tarball_path)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    archive.unpack(INSTALL_DIR)?;
    Ok(())
}
