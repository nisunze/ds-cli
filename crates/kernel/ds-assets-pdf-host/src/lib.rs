//! Native Assets text decoder, independent of the PDF.js visual renderer.
//! The caller supplies governed bytes and a page range, never a path, URL,
//! executable, environment, password or argv. Linux helpers run without a shell
//! under fixed resource limits; other hosts refuse explicitly.
#![forbid(unsafe_code)]

use serde::Serialize;

pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PAGES: u32 = 5;
pub const MAX_DOCUMENT_PAGES: u32 = 10_000;
pub const MAX_PAGE_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_TOTAL_TEXT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRange {
    pub first: u32,
    pub last: u32,
}

impl PageRange {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let (first, last) = value.split_once(':').ok_or_else(range_error)?;
        let parse = |value: &str| {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(range_error());
            }
            value.parse::<u32>().map_err(|_| range_error())
        };
        let range = Self {
            first: parse(first)?,
            last: parse(last)?,
        };
        range.validate()?;
        Ok(range)
    }

    pub fn validate(self) -> Result<(), Error> {
        if self.first == 0
            || self.last < self.first
            || self.last > MAX_DOCUMENT_PAGES
            || self.last - self.first >= MAX_PAGES
        {
            return Err(range_error());
        }
        Ok(())
    }
}

fn range_error() -> Error {
    Error::new("pdf_page_range_invalid", "PDF text needs an inclusive FIRST:LAST range, starting at 1, containing at most 5 pages and ending at most at 10000")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}
impl Error {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Serialize)]
pub struct PageText {
    pub page: u32,
    pub text: String,
    pub text_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct DocumentText {
    pub schema: &'static str,
    pub format: &'static str,
    pub total_pages: u32,
    pub first_page: u32,
    pub last_page: u32,
    pub pages: Vec<PageText>,
    pub total_text_bytes: usize,
    pub truncated: bool,
    pub notes: Vec<&'static str>,
}

pub fn extract(bytes: &[u8], range: PageRange) -> Result<DocumentText, Error> {
    range.validate()?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Error::new(
            "pdf_resource_limit",
            format!(
                "PDF input has {} bytes; bound is {MAX_INPUT_BYTES}",
                bytes.len()
            ),
        ));
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(Error::new(
            "pdf_invalid",
            "PDF bytes do not carry a PDF header",
        ));
    }
    #[cfg(target_os = "linux")]
    {
        linux::extract(bytes, range)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(Error::new(
            "pdf_text_unavailable",
            "bounded PDF text extraction requires the admitted Linux native host",
        ))
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        io::{Read, Write},
        path::Path,
        process::{Command, Stdio},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };

    const PDFINFO: &str = "/usr/bin/pdfinfo";
    const PDFTOTEXT: &str = "/usr/bin/pdftotext";
    const PRLIMIT: &str = "/usr/bin/prlimit";
    const STDERR_BYTES: usize = 16 * 1024;
    const INFO_BYTES: usize = 32 * 1024;
    const WALL: Duration = Duration::from_secs(8);

    // Private closed choice, not an executable or argument vector supplied
    // by a caller. Both decoders open only our generated private input file.
    #[derive(Clone, Copy)]
    enum Decode {
        Info,
        Page(u32),
    }

    fn unavailable() -> Error {
        Error::new("pdf_text_unavailable", "bounded PDF text extraction requires installed /usr/bin/{prlimit,pdfinfo,pdftotext} (util-linux and poppler-utils)")
    }
    fn resource(message: &str) -> Error {
        Error::new("pdf_resource_limit", message)
    }
    fn private_io() -> Error {
        Error::new(
            "pdf_decode_failed",
            "cannot stage or read the private PDF decoder input/output",
        )
    }

    pub(super) fn extract(bytes: &[u8], range: PageRange) -> Result<DocumentText, Error> {
        if [PDFINFO, PDFTOTEXT, PRLIMIT]
            .iter()
            .any(|tool| !Path::new(tool).is_file())
        {
            return Err(unavailable());
        }
        let directory = tempfile::Builder::new()
            .prefix("ds-assets-pdf-")
            .tempdir()
            .map_err(|_| private_io())?;
        let input = directory.path().join("input.pdf");
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&input)
            .map_err(|_| private_io())?;
        file.write_all(bytes).map_err(|_| private_io())?;
        drop(file);
        let deadline = Instant::now() + WALL;
        let info = decode(Decode::Info, &input, directory.path(), deadline)?;
        let total_pages = metadata(&info)?;
        if range.last > total_pages {
            return Err(Error::new(
                "pdf_page_range_invalid",
                format!(
                    "PDF has {total_pages} pages; requested {}:{}",
                    range.first, range.last
                ),
            ));
        }
        let mut pages = Vec::new();
        let mut total_text_bytes: usize = 0;
        for page in range.first..=range.last {
            let text = decode(Decode::Page(page), &input, directory.path(), deadline)?;
            // Poppler adds a final page separator. Remove only that framing,
            // preserving every extracted character and line otherwise.
            let text = text.strip_suffix('\u{c}').unwrap_or(&text).to_owned();
            if text.contains('\u{fffd}') || text.contains('\0') {
                return Err(Error::new("pdf_text_unsupported", "PDF text contains undecodable characters; no guessed replacement text is returned"));
            }
            let text_bytes = text.len();
            total_text_bytes = total_text_bytes
                .checked_add(text_bytes)
                .ok_or_else(|| resource("PDF text exceeds output bound"))?;
            if total_text_bytes > MAX_TOTAL_TEXT_BYTES {
                return Err(resource(
                    "PDF text exceeds the 262144-byte total output bound",
                ));
            }
            pages.push(PageText {
                page,
                text,
                text_bytes,
            });
        }
        Ok(DocumentText {
            schema: "ds.assets.pdf_text/v1", format: "pdf", total_pages,
            first_page: range.first, last_page: range.last, pages, total_text_bytes,
            truncated: false,
            notes: vec!["Embedded PDF text only; no OCR, remote content, annotations or visual layout reconstruction. An empty page means no extractable text, not an empty image."],
        })
    }

    fn metadata(info: &str) -> Result<u32, Error> {
        let mut pages = None;
        let mut encrypted = None;
        for line in info.lines() {
            if let Some(value) = line.strip_prefix("Pages:") {
                if pages.is_some() {
                    return Err(private_io());
                }
                pages = value.trim().parse::<u32>().ok();
            }
            if let Some(value) = line.strip_prefix("Encrypted:") {
                if encrypted.is_some() {
                    return Err(private_io());
                }
                encrypted = Some(value.trim().split_whitespace().next().unwrap_or(""));
            }
        }
        match encrypted {
            Some("yes") => {
                return Err(Error::new(
                    "pdf_encrypted",
                    "encrypted PDFs are refused; no password input is accepted",
                ))
            }
            Some("no") => (),
            _ => {
                return Err(Error::new(
                    "pdf_invalid",
                    "PDF decoder did not return a definite encryption state",
                ))
            }
        }
        match pages {
            Some(pages) if pages > MAX_DOCUMENT_PAGES => Err(Error::new(
                "pdf_resource_limit",
                format!("PDF has {pages} pages; document bound is {MAX_DOCUMENT_PAGES}"),
            )),
            Some(pages) if pages > 0 => Ok(pages),
            _ => Err(Error::new(
                "pdf_invalid",
                "PDF decoder did not return a positive page count",
            )),
        }
    }

    fn drain(
        mut pipe: impl Read,
        cap: usize,
        exceeded: Arc<AtomicBool>,
    ) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let count = pipe.read(&mut buffer)?;
            if count == 0 {
                return Ok(bytes);
            }
            if count > cap.saturating_sub(bytes.len()) {
                exceeded.store(true, Ordering::SeqCst);
                return Ok(bytes);
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
    }

    fn decode(
        kind: Decode,
        input: &Path,
        directory: &Path,
        deadline: Instant,
    ) -> Result<String, Error> {
        if Instant::now() >= deadline {
            return Err(resource("PDF text exceeded the shared 8-second deadline"));
        }
        let mut command = Command::new(PRLIMIT);
        command.args([
            "--as=268435456",
            "--cpu=5",
            "--fsize=262144",
            "--nofile=32",
            "--core=0",
            "--",
        ]);
        let cap = match kind {
            Decode::Info => {
                command.args([PDFINFO, "-enc", "UTF-8"]);
                INFO_BYTES
            }
            Decode::Page(page) => {
                command.args([
                    PDFTOTEXT,
                    "-f",
                    &page.to_string(),
                    "-l",
                    &page.to_string(),
                    "-enc",
                    "UTF-8",
                ]);
                MAX_PAGE_TEXT_BYTES
            }
        };
        command.arg(input);
        if matches!(kind, Decode::Page(_)) {
            command.arg("-");
        }
        command
            .env_clear()
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("HOME", directory)
            .env("TMPDIR", directory)
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let mut child = command.spawn().map_err(|_| unavailable())?;
        let stdout = child.stdout.take().expect("requested stdout pipe");
        let stderr = child.stderr.take().expect("requested stderr pipe");
        let exceeded = Arc::new(AtomicBool::new(false));
        let out_limit = exceeded.clone();
        let err_limit = exceeded.clone();
        let out_reader = std::thread::spawn(move || drain(stdout, cap, out_limit));
        let err_reader = std::thread::spawn(move || drain(stderr, STDERR_BYTES, err_limit));
        let status = loop {
            if exceeded.load(Ordering::SeqCst) || Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break Err(resource(
                    "PDF decoder exceeded its shared deadline or output bound",
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(private_io());
                }
            }
        };
        let stdout = out_reader
            .join()
            .map_err(|_| private_io())?
            .map_err(|_| private_io())?;
        let stderr = err_reader
            .join()
            .map_err(|_| private_io())?
            .map_err(|_| private_io())?;
        if exceeded.load(Ordering::SeqCst) {
            return Err(resource("PDF decoder exceeded an output bound"));
        }
        let status = status?;
        let errors = String::from_utf8_lossy(&stderr).to_ascii_lowercase();
        if errors.contains("incorrect password") {
            return Err(Error::new(
                "pdf_encrypted",
                "encrypted PDFs are refused; no password input is accepted",
            ));
        }
        // No partial text after malformed content/font warnings. Do not expose
        // helper diagnostics (which can contain document content or paths).
        if !status.success() || !stderr.is_empty() {
            use std::os::unix::process::ExitStatusExt;
            if status.signal().is_some() {
                return Err(resource("PDF decoder terminated under its resource limits"));
            }
            return Err(Error::new(
                "pdf_invalid",
                "PDF decoder refused malformed or unsupported document content",
            ));
        }
        String::from_utf8(stdout).map_err(|_| {
            Error::new(
                "pdf_text_unsupported",
                "PDF text decoder did not return valid UTF-8",
            )
        })
    }

    #[cfg(test)]
    mod limits_tests {
        use super::*;

        #[test]
        fn metadata_encryption_and_page_count_are_closed() {
            assert_eq!(metadata("Pages: 2\nEncrypted: no\n").unwrap(), 2);
            assert_eq!(
                metadata("Pages: 2\nEncrypted: yes (print:yes)\n")
                    .unwrap_err()
                    .code,
                "pdf_encrypted"
            );
            for info in [
                "Pages: 0\nEncrypted: no\n",
                "Pages: 1\n",
                "Pages: x\nEncrypted: no\n",
                "Pages: 1\nEncrypted: maybe\n",
            ] {
                assert_eq!(metadata(info).unwrap_err().code, "pdf_invalid");
            }
            assert_eq!(
                metadata("Pages: 10001\nEncrypted: no\n").unwrap_err().code,
                "pdf_resource_limit"
            );
        }

        #[test]
        fn pipe_cap_and_shared_expired_deadline_refuse_without_partial_text() {
            let exceeded = Arc::new(AtomicBool::new(false));
            assert_eq!(
                drain(std::io::Cursor::new(b"abcd"), 4, exceeded.clone()).unwrap(),
                b"abcd"
            );
            assert!(!exceeded.load(Ordering::SeqCst));
            drain(std::io::Cursor::new(b"abcde"), 4, exceeded.clone()).unwrap();
            assert!(exceeded.load(Ordering::SeqCst));
            assert_eq!(
                decode(
                    Decode::Info,
                    Path::new("/nonexistent"),
                    Path::new("/nonexistent"),
                    Instant::now() - Duration::from_secs(1)
                )
                .unwrap_err()
                .code,
                "pdf_resource_limit"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real, small PDF with an explicit ToUnicode mapping. Independent of
    // a renderer or external fixture generator; offsets/stream lengths are
    // computed from exact bytes, including non-ASCII expected strings.
    fn fixture(texts: &[&str]) -> Vec<u8> {
        let font_id = 3 + texts.len() * 2;
        let cmap_id = font_id + 1;
        let mut characters = std::collections::BTreeSet::new();
        for text in texts {
            characters.extend(text.chars());
        }
        assert!(characters.len() < 256);
        let mut next_unicode = 128;
        let codes: std::collections::BTreeMap<_, _> = characters
            .into_iter()
            .map(|c| {
                let code = if c.is_ascii() {
                    c as usize
                } else {
                    let code = next_unicode;
                    next_unicode += 1;
                    code
                };
                (c, code)
            })
            .collect();
        let stream = |content: &str| {
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            )
        };
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Count {} /Kids [{}] >>",
                texts.len(),
                (0..texts.len())
                    .map(|i| format!("{} 0 R", 3 + i * 2))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        ];
        for (i, text) in texts.iter().enumerate() {
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1000000 800] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {} 0 R >>", 4 + i * 2));
            let encoded = text
                .chars()
                .map(|c| format!("{:02x}", codes[&c]))
                .collect::<String>();
            objects.push(stream(&format!("BT /F1 12 Tf 20 700 Td <{encoded}> Tj ET")));
        }
        objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode {cmap_id} 0 R >>"));
        let mappings = codes
            .iter()
            .map(|(c, code)| format!("<{code:02x}> <{:04x}>", *c as u32))
            .collect::<Vec<_>>()
            .join("\n");
        objects.push(stream(&format!("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /DSUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<00> <ff>\nendcodespacerange\n{} beginbfchar\n{mappings}\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend", codes.len())));
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes(),
        );
        for offset in &offsets[1..] {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn page_range_is_closed_and_bounded_before_any_decoder() {
        assert_eq!(
            PageRange::parse("2:4").unwrap(),
            PageRange { first: 2, last: 4 }
        );
        for range in [
            "0:1",
            "2:1",
            "1:6",
            "1",
            "1:2:3",
            "1:10001",
            "-1:1",
            "1 :2",
            "1:4294967296",
        ] {
            assert_eq!(
                PageRange::parse(range).unwrap_err().code,
                "pdf_page_range_invalid",
                "{range}"
            );
        }
        assert_eq!(
            extract(b"not a PDF", PageRange { first: 1, last: 1 })
                .unwrap_err()
                .code,
            "pdf_invalid"
        );
        assert_eq!(
            extract(
                &vec![0; MAX_INPUT_BYTES + 1],
                PageRange { first: 1, last: 1 }
            )
            .unwrap_err()
            .code,
            "pdf_resource_limit"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn actual_unicode_pdf_preserves_exact_pages_and_subset() {
        let bytes = fixture(&["Gisagara — Rwanda", "Kigali αβΩ"]);
        let all = extract(&bytes, PageRange { first: 1, last: 2 }).unwrap();
        assert_eq!(all.total_pages, 2);
        assert_eq!(all.pages[0].text.trim(), "Gisagara — Rwanda");
        assert_eq!(all.pages[1].text.trim(), "Kigali αβΩ");
        assert_eq!(
            all.total_text_bytes,
            all.pages.iter().map(|p| p.text.len()).sum::<usize>()
        );
        assert!(!all.truncated);
        let subset = extract(&bytes, PageRange { first: 2, last: 2 }).unwrap();
        assert_eq!(subset.pages.len(), 1);
        assert_eq!(subset.pages[0].page, 2);
        assert_eq!(subset.pages[0].text, all.pages[1].text);
        assert_eq!(
            extract(&bytes, PageRange { first: 2, last: 3 })
                .unwrap_err()
                .code,
            "pdf_page_range_invalid"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn invalid_pdf_and_actual_excess_output_are_refused_not_partial() {
        assert_eq!(
            extract(b"%PDF-1.4\ninvalid\n", PageRange { first: 1, last: 1 })
                .unwrap_err()
                .code,
            "pdf_invalid"
        );
        let text = "word ".repeat(20_000);
        assert_eq!(
            extract(&fixture(&[&text]), PageRange { first: 1, last: 1 })
                .unwrap_err()
                .code,
            "pdf_resource_limit"
        );
        let text = "word ".repeat(12_000);
        assert_eq!(
            extract(
                &fixture(&[text.as_str(); 5]),
                PageRange { first: 1, last: 5 }
            )
            .unwrap_err()
            .code,
            "pdf_resource_limit"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn real_encrypted_pdfs_refuse_even_with_empty_password() {
        // Synthetic one-page PDF Standard Security R2 fixtures; no project
        // bytes, secrets, or external generator dependency are retained.
        for pdf in [
            include_bytes!("../tests/fixtures/encrypted-empty-password.pdf").as_slice(),
            include_bytes!("../tests/fixtures/encrypted-password.pdf").as_slice(),
        ] {
            assert_eq!(
                extract(pdf, PageRange { first: 1, last: 1 })
                    .unwrap_err()
                    .code,
                "pdf_encrypted"
            );
        }
    }

    #[test]
    #[ignore = "requires exact sample previously read by installed ds assets read"]
    #[cfg(target_os = "linux")]
    fn governed_sample_receipt() {
        let path = std::env::var("DS_ASSETS_PDF_TEST_SAMPLE").expect("named governed sample path");
        let bytes = std::fs::read(path).unwrap();
        let doc = extract(&bytes, PageRange { first: 1, last: 1 }).unwrap();
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.pages[0].page, 1);
        assert!(!doc.pages[0].text.trim().is_empty());
        assert!(!doc.truncated);
        println!(
            "governed sample: bytes={} total_pages={} page=1 text_bytes={} truncated=false",
            bytes.len(),
            doc.total_pages,
            doc.total_text_bytes
        );
    }
}
