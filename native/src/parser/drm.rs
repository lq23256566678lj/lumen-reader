use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::BookError;
use crate::state::models::BookFormat;

/// Rejects known DRM containers before handing bytes to a parser. We
/// deliberately do not attempt to remove or bypass any protection.
pub fn check(path: &Path, format: BookFormat) -> Result<(), BookError> {
    match format {
        BookFormat::Epub | BookFormat::Cbz => check_zip(path, format),
        BookFormat::Mobi | BookFormat::Azw | BookFormat::Azw3 => check_mobi(path),
        BookFormat::Pdf => check_pdf(path),
        BookFormat::Fb2 | BookFormat::Txt => Ok(()),
    }
}

fn check_zip(path: &Path, format: BookFormat) -> Result<(), BookError> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        let text = error.to_string();
        if text.to_ascii_lowercase().contains("password")
            || text.to_ascii_lowercase().contains("encrypt")
        {
            BookError::DrmProtected(format!("{format:?} 加密压缩包"))
        } else {
            BookError::Archive(text)
        }
    })?;

    for index in 0..archive.len() {
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => return Err(BookError::Archive(error.to_string())),
        };
        let name = match entry.name() {
            Ok(name) => name.to_ascii_lowercase(),
            Err(error) => return Err(BookError::Archive(error.to_string())),
        };

        if name.ends_with("meta-inf/rights.xml") {
            return Err(BookError::DrmProtected("Adobe ADEPT".to_owned()));
        }
        if name.ends_with("meta-inf/license.lcp")
            || name.ends_with("meta-inf/license.lcpl")
            || name.ends_with(".lcp")
            || name.ends_with(".lcpl")
        {
            return Err(BookError::DrmProtected("Readium LCP".to_owned()));
        }

        if name.ends_with("meta-inf/encryption.xml") {
            let mut xml = String::new();
            entry
                .read_to_string(&mut xml)
                .map_err(|error| BookError::Archive(error.to_string()))?;
            if encryption_xml_is_content_drm(&xml) {
                return Err(BookError::DrmProtected("EPUB 内容加密".to_owned()));
            }
        }
    }

    Ok(())
}

/// EPUB font obfuscation is not content DRM and is allowed. Any encrypted
/// resource that is not a font, or any unknown encryption block, is rejected.
fn encryption_xml_is_content_drm(xml: &str) -> bool {
    let lower = xml.to_ascii_lowercase();
    if !lower.contains("<encrypteddata") {
        return false;
    }

    for block in lower.split("<encrypteddata").skip(1) {
        let mentions_font = block.contains("font-obfuscation")
            || block.contains(".ttf")
            || block.contains(".otf")
            || block.contains(".woff")
            || block.contains(".woff2");
        if !mentions_font {
            return true;
        }
    }
    false
}

fn check_mobi(path: &Path) -> Result<(), BookError> {
    let mut file = File::open(path)?;
    let mut head = vec![0_u8; 256 * 1024];
    let read = file.read(&mut head)?;
    head.truncate(read);
    if head.len() < 86 {
        return Ok(());
    }

    let num_records = u16::from_be_bytes([head[76], head[77]]) as usize;
    if num_records == 0 {
        return Ok(());
    }
    let record0 = u32::from_be_bytes([head[78], head[79], head[80], head[81]]) as usize;
    if record0 + 16 > head.len() {
        return Ok(());
    }

    // PalmDOC header is 16 bytes; the encryption type sits at rec0[12..14].
    let encryption = u16::from_be_bytes([head[record0 + 12], head[record0 + 13]]);
    if encryption != 0 {
        return Err(BookError::DrmProtected(
            "Mobipocket / Kindle DRM".to_owned(),
        ));
    }

    // Some DRM'd AZW files have no readable MOBI header. A BOOKMOBI marker is
    // still a useful sanity check, but the encryption field above is normative.
    Ok(())
}

fn check_pdf(path: &Path) -> Result<(), BookError> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if length < 8 {
        return Ok(());
    }

    let mut header = [0_u8; 4];
    file.read_exact(&mut header)?;
    if &header != b"%PDF" {
        return Ok(());
    }

    // Scan in chunks with a small overlap so `/Encrypt` cannot straddle a
    // buffer boundary. `/Encrypt` appearing inside a content stream is rare
    // enough that a false positive can simply be re-saved without encryption.
    file.seek(SeekFrom::Start(0))?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let mut chunk = Vec::with_capacity(carry.len() + read);
        chunk.extend_from_slice(&carry);
        chunk.extend_from_slice(&buffer[..read]);
        if contains_bytes(&chunk, b"/Encrypt") {
            return Err(BookError::PdfEncrypted);
        }
        let keep = chunk.len().min(16);
        carry = chunk[chunk.len() - keep..].to_vec();
    }
    Ok(())
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_obfuscation_is_not_content_drm() {
        let xml = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
          <EncryptedData>
            <CipherData><CipherReference URI="OEBPS/fonts/serif.otf"/></CipherData>
            <EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>
          </EncryptedData>
        </encryption>"#;
        assert!(!encryption_xml_is_content_drm(xml));
    }

    #[test]
    fn content_encryption_is_rejected() {
        let xml = r#"<encryption>
          <EncryptedData>
            <CipherData><CipherReference URI="OEBPS/chapter1.xhtml"/></CipherData>
          </EncryptedData>
        </encryption>"#;
        assert!(encryption_xml_is_content_drm(xml));
    }

    #[test]
    fn mobi_encryption_header_is_detected() {
        let path = std::env::temp_dir().join(format!(
            "lumen-mobi-drm-{}-{}.mobi",
            std::process::id(),
            crate::state::models::now_secs()
        ));
        let mut data = vec![0_u8; 512];
        data[76..78].copy_from_slice(&1_u16.to_be_bytes());
        data[78..82].copy_from_slice(&90_u32.to_be_bytes());
        data[90 + 12..90 + 14].copy_from_slice(&2_u16.to_be_bytes());
        let write_result = std::fs::write(&path, data);
        assert!(write_result.is_ok());
        let result = check(&path, BookFormat::Mobi);
        let _ = std::fs::remove_file(&path);
        assert!(matches!(result, Err(BookError::DrmProtected(_))));
    }
}
