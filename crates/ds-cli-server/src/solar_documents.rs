//! Fixed local document-tool effects. Solar owns sources, media and format admission.
use ds_solar_native::{ReportFormat, ReportRenderer};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) struct NativeDocuments {
    pub directory: PathBuf,
    pub owner: String,
    pub lane: String,
    pub auth: Arc<dyn ds_compute_runtime::Authorizer>,
}

fn tool(name: &str) -> Result<PathBuf, String> {
    let mut locations: Vec<_> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        locations.push(PathBuf::from(home).join(".local/bin"));
    }
    locations
        .into_iter()
        .map(|directory| directory.join(name))
        .find(|path| path.is_absolute() && path.is_file())
        .ok_or_else(|| format!("Headless report finishing requires installed {name}"))
}
fn private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Cannot create private report resource")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot write private report resource".into())
}
fn execute(mut command: Command, name: &str) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| format!("Cannot start installed {name}"))?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "Installed {name} refused the verified report presentation copy"
                    ))
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                #[cfg(unix)]
                {
                    // The group is created by this invocation and contains only its tool children.
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Installed {name} exceeded the bounded report finishing time"
                ));
            }
        }
    }
}
fn read_document(path: &Path) -> Result<Vec<u8>, String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "The document tool produced no output")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 * 1024
    {
        return Err("The document tool output is invalid or oversized".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "Cannot open the finished report")?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the finished report")?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("The finished report exceeds 64 MiB".into());
    }
    Ok(bytes)
}

impl ReportRenderer for NativeDocuments {
    fn render(
        &self,
        copy: ds_command_kernel::solar_report_bundle::RenderCopy,
        format: ReportFormat,
    ) -> Result<Vec<u8>, String> {
        self.auth.authorize(&self.owner)?;
        if crate::auth::owner_fence(&self.lane)? != self.owner {
            return Err("Server owner changed before report finishing".into());
        }
        let bytes = render_document(&self.directory, copy, format)?;
        self.auth.authorize(&self.owner)?;
        if crate::auth::owner_fence(&self.lane)? != self.owner {
            return Err("Server owner changed during report finishing".into());
        }
        Ok(bytes)
    }
}

fn render_document(
    directory: &Path,
    copy: ds_command_kernel::solar_report_bundle::RenderCopy,
    format: ReportFormat,
) -> Result<Vec<u8>, String> {
    let pandoc = tool("pandoc")?;
    let office = if matches!(format, ReportFormat::Pdf) {
        Some(tool("libreoffice")?)
    } else {
        None
    };
    let parent = ds_solar_native::workspace::secure_private_directory(directory)?;
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce)
        .map_err(|_| "Cannot identify a private document-tool invocation")?;
    let directory = parent.join(
        nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    );
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&directory)
        .map_err(|_| "Cannot create private document-tool staging")?;
    struct Staging(PathBuf);
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _guard = Staging(directory.clone());
    private_file(&directory.join("report.md"), copy.markdown.as_bytes())?;
    for (name, bytes) in copy.media {
        let relative = Path::new(&name);
        if !name.starts_with("media/")
            || !relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err("The report renderer received an unsafe resource name".into());
        }
        let path = directory.join(relative);
        ds_solar_native::workspace::secure_private_directory(
            path.parent().ok_or("Report resource has no parent")?,
        )?;
        private_file(&path, &bytes)?;
    }
    let mut command = Command::new(pandoc);
    command.current_dir(&directory).args([
        "--from=markdown-raw_html-raw_tex",
        "--to=docx",
        "--standalone",
        "--output=report.docx",
        "report.md",
    ]);
    execute(command, "Pandoc")?;
    let name = if let Some(office) = office {
        let profile = directory.join("office-profile");
        let path = profile
            .to_str()
            .ok_or("The document-tool profile is not UTF-8")?;
        let escaped = path
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"/._-".contains(&byte) {
                    (byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect::<String>();
        let mut command = Command::new(office);
        command
            .current_dir(&directory)
            .arg(format!("-env:UserInstallation=file://{escaped}"))
            .args([
                "--headless",
                "--convert-to",
                "pdf:writer_pdf_Export",
                "--outdir",
                ".",
                "report.docx",
            ]);
        execute(command, "LibreOffice")?;
        "report.pdf"
    } else {
        "report.docx"
    };
    let bytes = read_document(&directory.join(name))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires locally installed Pandoc and LibreOffice"]
    fn installed_tools_finish_word_with_embedded_image_and_pdf() {
        use base64::Engine;
        let directory = tempfile::tempdir().unwrap();
        let image = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
        let copy = || ds_command_kernel::solar_report_bundle::RenderCopy {
            markdown:
                "# Headless Solar fixture\n\nReviewed fixture text.\n\n![Chart](media/chart.png)\n"
                    .into(),
            media: std::collections::BTreeMap::from([("media/chart.png".into(), image.clone())]),
        };
        let docx = render_document(directory.path(), copy(), ReportFormat::Docx).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(docx)).unwrap();
        assert!(
            archive
                .file_names()
                .any(|name| name.starts_with("word/media/"))
        );
        let mut document = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document)
            .unwrap();
        assert!(
            document.contains("Headless Solar fixture")
                && document.contains("Reviewed fixture text.")
        );
        let pdf = render_document(directory.path(), copy(), ReportFormat::Pdf).unwrap();
        assert!(pdf.starts_with(b"%PDF-") && pdf.len() > 1000);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
