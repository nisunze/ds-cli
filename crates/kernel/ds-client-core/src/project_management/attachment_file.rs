//! One file a Project Work attachment uploads.
//!
//! A local file is never read whole into memory: it is measured and hashed
//! once in bounded reads when the attachment is made, then opened again and
//! streamed to the resumable storage session, hashed a second time on the
//! way. Both passes must name the same bytes before the upload is reported,
//! so a file changed in between refuses instead of attaching other bytes
//! under the first digest. In-memory bytes remain for callers that already
//! hold a small document.

use super::{attachment_media_type, MAX_ATTACHMENT_BYTES};
use crate::{ClientError, ErrorKind};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// A stored attachment name is at most this many UTF-8 bytes.
const MAX_ATTACHMENT_NAME_BYTES: usize = 255;
/// One bounded read while hashing or streaming a local file.
const READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq)]
enum Source {
    Bytes(Vec<u8>),
    File(PathBuf),
}

/// One file a task or record attachment uploads, checked once when it is
/// made: a plain file name, 1 byte up to Cloud Storage's 5 TiB object
/// maximum, and a media type (given, or named from the file extension the way
/// a browser names `File.type`). `Debug` prints the size, never the bytes or
/// the local path.
#[derive(Clone, PartialEq)]
pub struct AttachmentFile {
    name: String,
    content_type: String,
    source: Source,
    len: u64,
    sha256: String,
}

impl std::fmt::Debug for AttachmentFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttachmentFile")
            .field("name", &self.name)
            .field("content_type", &self.content_type)
            .field("bytes", &self.len)
            .finish()
    }
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

const SIZE_RULE: &str = "a task attachment is from 1 byte to 5 TiB (the storage maximum)";
const FILE_RULE: &str =
    "a task attachment is one regular file from 1 byte to 5 TiB (the storage maximum)";

impl AttachmentFile {
    /// An attachment over bytes the caller already holds.
    pub fn new(
        name: &str,
        content_type: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<Self, ClientError> {
        let (name, content_type) = checked(name, content_type)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err(invalid(SIZE_RULE));
        }
        Ok(Self {
            name,
            content_type,
            len: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            source: Source::Bytes(bytes),
        })
    }

    /// An attachment over one local regular file, measured and hashed in
    /// bounded reads. The file stays on disk; only its path is kept.
    pub fn from_path(
        name: &str,
        content_type: Option<&str>,
        path: &Path,
    ) -> Result<Self, ClientError> {
        let (name, content_type) = checked(name, content_type)?;
        let unreadable = |_: std::io::Error| invalid("the file could not be read");
        let metadata = std::fs::metadata(path).map_err(unreadable)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ATTACHMENT_BYTES {
            return Err(invalid(FILE_RULE));
        }
        let mut hashing = Verifying::new(std::fs::File::open(path).map_err(unreadable)?);
        std::io::copy(&mut hashing, &mut std::io::sink()).map_err(unreadable)?;
        let (len, sha256) = hashing.finish();
        if len != metadata.len() {
            return Err(invalid("the file changed while it was being read"));
        }
        Ok(Self {
            name,
            content_type,
            source: Source::File(path.to_owned()),
            len,
            sha256,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn content_type(&self) -> &str {
        &self.content_type
    }
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Lowercase hex SHA-256 of the bytes this attachment uploads.
    pub fn sha256(&self) -> String {
        self.sha256.clone()
    }

    /// A fresh bounded reader over the same bytes, hashed as it is read.
    pub(crate) fn open(&self) -> Result<Verifying<Box<dyn Read + '_>>, ClientError> {
        let reader: Box<dyn Read + '_> = match &self.source {
            Source::Bytes(bytes) => Box::new(bytes.as_slice()),
            Source::File(path) => Box::new(
                std::fs::File::open(path)
                    .map_err(|_| invalid("the file could not be reopened for upload"))?,
            ),
        };
        Ok(Verifying::new(reader))
    }

    /// True when a completed read named exactly this attachment's bytes.
    pub(crate) fn same_bytes(&self, read: (u64, String)) -> bool {
        read.0 == self.len && read.1 == self.sha256
    }
}

/// Counts and hashes every byte read through it, in the caller's buffer.
pub(crate) struct Verifying<R> {
    inner: R,
    digest: Sha256,
    read: u64,
}

impl<R: Read> Verifying<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            read: 0,
        }
    }
    /// The byte count and lowercase hex SHA-256 of everything read.
    pub(crate) fn finish(self) -> (u64, String) {
        (self.read, format!("{:x}", self.digest.finalize()))
    }
}

impl<R: Read> Read for Verifying<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let limit = buffer.len().min(READ_BUFFER_BYTES);
        let count = self.inner.read(&mut buffer[..limit])?;
        self.digest.update(&buffer[..count]);
        self.read += count as u64;
        Ok(count)
    }
}

/// A plain file name of at most 255 UTF-8 bytes and one `type/subtype`.
fn checked(name: &str, content_type: Option<&str>) -> Result<(String, String), ClientError> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > MAX_ATTACHMENT_NAME_BYTES
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(invalid(
            "a task attachment needs a plain file name of at most 255 UTF-8 bytes",
        ));
    }
    let content_type = match content_type.map(str::trim) {
        Some(given) => {
            let lowered = given.to_ascii_lowercase();
            let valid = !lowered.is_empty()
                && lowered.len() <= 200
                && lowered.split_once('/').is_some_and(|(kind, sub)| {
                    !kind.is_empty() && !sub.is_empty() && !sub.contains('/')
                })
                && !lowered
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == ';');
            if !valid {
                return Err(invalid(
                    "a task attachment content type is one `type/subtype`",
                ));
            }
            lowered
        }
        None => attachment_media_type(name).to_owned(),
    };
    Ok((name.to_owned(), content_type))
}
