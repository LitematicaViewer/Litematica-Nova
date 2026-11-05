//! Indexed column dictionaries for four default oracle tables. A logical row is
//! the concatenation of its original UTF-8 spans, including its original newline.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

fn layout(path: &Path) -> Option<(u8, usize)> {
    if path.extension()? != "jsonl" {
        return None;
    }
    Some(match path.file_stem()?.to_str()? {
        "skip_render_rows" => (1, 3),
        "block_state_semantics" => (2, 7),
        "block_states" => (3, 9),
        "tags" => (4, 5),
        _ => return None,
    })
}
pub fn available_name(path: &Path) -> bool {
    layout(path).is_some()
}

pub struct CompactTable {
    columns: Vec<Vec<String>>,
    refs: Vec<u32>,
    bytes: u64,
    digest: [u8; 32],
    kind: u8,
}

fn take<'a>(src: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    let (head, rest) = src
        .split_at_checked(n)
        .context("truncated compact oracle")?;
    *src = rest;
    Ok(head)
}
fn number(src: &mut &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(take(src, 4)?.try_into()?))
}

pub fn open(path: &Path) -> Result<Option<CompactTable>> {
    let Some((kind, width)) = layout(path) else {
        return Ok(None);
    };
    let location = path.with_extension("novatab");
    let mut file = match File::open(&location) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let read = || -> Result<CompactTable> {
        const LIMIT: usize = 64 << 20;
        let packed_len = file.metadata()?.len();
        ensure!(
            packed_len >= 44 && packed_len <= LIMIT as u64,
            "invalid compact oracle size"
        );
        let mut header = [0; 44];
        file.read_exact(&mut header)?;
        ensure!(&header[..8] == b"NVTAB001", "unknown compact oracle schema");
        let size = u32::from_le_bytes(header[8..12].try_into()?) as usize;
        ensure!(size <= LIMIT, "compact oracle exceeds storage boundary");
        let mut packed = Vec::with_capacity((packed_len - 44) as usize);
        file.take(packed_len - 43).read_to_end(&mut packed)?;
        ensure!(
            packed.len() as u64 == packed_len - 44,
            "compact oracle changed while reading"
        );
        let mut body = Vec::with_capacity(size + 1);
        let mut decoder = flate2::Decompress::new(true);
        let end = decoder.decompress_vec(&packed, &mut body, flate2::FlushDecompress::Finish)?;
        ensure!(
            end == flate2::Status::StreamEnd
                && decoder.total_in() == packed.len() as u64
                && body.len() == size
                && Sha256::digest(&body)[..] == header[12..],
            "compact oracle integrity mismatch"
        );
        drop(packed);
        let mut src = body.as_slice();
        ensure!(
            take(&mut src, 1)?[0] == kind,
            "compact oracle belongs to a different table"
        );
        let rows = number(&mut src)? as usize;
        let bytes = u64::from_le_bytes(take(&mut src, 8)?.try_into()?);
        let digest = take(&mut src, 32)?.try_into()?;
        ensure!(
            rows <= 1_000_000 && bytes <= 1 << 30,
            "compact oracle logical size out of range"
        );
        let mut columns = Vec::with_capacity(width);
        for _ in 0..width {
            let n = number(&mut src)? as usize;
            ensure!(
                n <= rows && n <= src.len() / 4,
                "compact oracle dictionary too large"
            );
            let mut col = Vec::with_capacity(n);
            for _ in 0..n {
                let len = number(&mut src)? as usize;
                col.push(std::str::from_utf8(take(&mut src, len)?)?.to_owned());
            }
            columns.push(col);
        }
        let count = rows
            .checked_mul(width)
            .context("oracle row index overflow")?;
        ensure!(
            count.checked_mul(4) == Some(src.len()),
            "invalid compact oracle row index"
        );
        let mut refs = Vec::with_capacity(count);
        let mut actual = 0u64;
        for i in 0..count {
            let id = number(&mut src)?;
            let cell = columns[i % width]
                .get(id as usize)
                .context("compact oracle reference outside dictionary")?;
            actual = actual
                .checked_add(cell.len() as u64)
                .context("oracle logical length overflow")?;
            refs.push(id);
        }
        ensure!(actual == bytes, "compact oracle logical length differs");
        Ok(CompactTable {
            columns,
            refs,
            bytes,
            digest,
            kind,
        })
    };
    read()
        .map(Some)
        .with_context(|| format!("reading {}", location.display()))
}

impl CompactTable {
    pub fn logical_len(&self) -> u64 {
        self.bytes
    }
    pub fn row_count(&self) -> usize {
        self.refs.len() / self.columns.len()
    }
    fn cell(&self, n: usize) -> &str {
        &self.columns[n % self.columns.len()][self.refs[n] as usize]
    }
    pub fn row_parts(&self, row: usize) -> Result<impl Iterator<Item = &[u8]>> {
        ensure!(row < self.row_count(), "compact oracle row out of bounds");
        let width = self.columns.len();
        Ok((row * width..(row + 1) * width).map(|i| self.cell(i).as_bytes()))
    }
    pub fn skip_pool(&self) -> Result<&[String]> {
        ensure!(self.kind == 1, "not a skip-render table");
        Ok(&self.columns[1])
    }
    pub fn skip_row(&self, row: usize) -> Result<(&str, usize, &str)> {
        ensure!(
            self.kind == 1 && row < self.row_count(),
            "invalid skip-render row"
        );
        let i = row * 3;
        Ok((self.cell(i), self.refs[i + 1] as usize, self.cell(i + 2)))
    }
    /// Used once by the importer to bind the new representation to the trusted original export.
    pub(super) fn verified_record(&self) -> Result<super::ResourceRecord> {
        let mut hash = Sha256::new();
        for i in 0..self.row_count() {
            for part in self.row_parts(i)? {
                hash.update(part);
            }
        }
        let digest = hash.finalize();
        ensure!(
            digest[..] == self.digest,
            "logical oracle digest differs from its source"
        );
        Ok(super::ResourceRecord {
            bytes: self.bytes,
            sha256: format!("{digest:x}"),
        })
    }
    pub fn into_reader(self) -> impl Read + Send {
        struct Reader {
            table: CompactTable,
            span: usize,
            offset: usize,
        }
        impl Read for Reader {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let mut n = 0;
                while n < out.len() && self.span < self.table.refs.len() {
                    let part = self.table.cell(self.span).as_bytes();
                    let len = (part.len() - self.offset).min(out.len() - n);
                    out[n..n + len].copy_from_slice(&part[self.offset..self.offset + len]);
                    self.offset += len;
                    n += len;
                    if self.offset == part.len() {
                        self.span += 1;
                        self.offset = 0;
                    }
                }
                Ok(n)
            }
        }
        Reader {
            table: self,
            span: 0,
            offset: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    // Use actual generator outputs and the independent original version export (not golden copies).
    #[test]
    #[ignore = "set NOVA_RAW_ORACLE_DIR and NOVA_COMPACT_ORACLE_DIR to the two real layouts"]
    fn compare_real_oracle_and_reject_invalid_references() -> Result<()> {
        let raw = std::path::PathBuf::from(
            std::env::var_os("NOVA_RAW_ORACLE_DIR").context("original oracle")?,
        );
        let packed = std::path::PathBuf::from(
            std::env::var_os("NOVA_COMPACT_ORACLE_DIR").context("generated oracle")?,
        );
        for name in [
            "skip_render_rows",
            "block_state_semantics",
            "block_states",
            "tags",
        ] {
            let logical = format!("{name}.jsonl");
            let table = open(&packed.join(&logical))?.context("missing compact table")?;
            let mut original = BufReader::new(File::open(raw.join(&logical))?);
            let mut line = Vec::new();
            for row in 0..table.row_count() {
                line.clear();
                ensure!(
                    original.read_until(b'\n', &mut line)? != 0,
                    "original oracle too short"
                );
                let mut rest = line.as_slice();
                for piece in table.row_parts(row)? {
                    rest = rest
                        .strip_prefix(piece)
                        .context("logical oracle row differs")?;
                }
                ensure!(rest.is_empty(), "short logical oracle row");
            }
            ensure!(original.fill_buf()?.is_empty(), "extra original rows");
            table.verified_record()?;
        }

        let name = "skip_render_rows";
        let mut source = std::fs::read(packed.join(format!("{name}.novatab")))?;
        let tmp = std::env::temp_dir().join(format!(
            "nova-compact-oracle-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&tmp)?;
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(self.0.join("skip_render_rows.novatab"));
                let _ = std::fs::remove_dir(&self.0);
            }
        }
        let _guard = Cleanup(tmp.clone());
        let path = tmp.join(format!("{name}.novatab"));
        std::fs::write(&path, &source[..source.len() - 1])?;
        ensure!(
            open(&path.with_extension("jsonl")).is_err(),
            "truncation was accepted"
        );
        let mut body = Vec::new();
        flate2::read::ZlibDecoder::new(&source[44..]).read_to_end(&mut body)?;
        let n = body.len();
        body[n - 4..].fill(0xff); // Out-of-range slot even though the new physical checksum is correct.
        source.truncate(44);
        source[12..44].copy_from_slice(&Sha256::digest(&body));
        let mut encoder = flate2::write::ZlibEncoder::new(source, flate2::Compression::fast());
        encoder.write_all(&body)?;
        std::fs::write(&path, encoder.finish()?)?;
        ensure!(
            open(&path.with_extension("jsonl")).is_err(),
            "bad dictionary reference was accepted"
        );
        Ok(())
    }
}
