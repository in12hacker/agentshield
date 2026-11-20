//! Mature streaming content decoders with a bounded output writer.
use std::io::{self, Write};
pub(crate) struct Output {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(buf.len()) > self.limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "decode_budget"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) enum Content {
    Identity,
    Gzip(Box<flate2::write::MultiGzDecoder<Output>>),
    Brotli(Box<brotli::DecompressorWriter<Output>>),
}
impl Content {
    pub fn new(encoding: &str, limit: usize) -> Option<Self> {
        let output = Output {
            bytes: Vec::new(),
            limit,
        };
        match encoding {
            "identity" | "" => Some(Self::Identity),
            "gzip" => Some(Self::Gzip(Box::new(flate2::write::MultiGzDecoder::new(
                output,
            )))),
            "br" => Some(Self::Brotli(Box::new(brotli::DecompressorWriter::new(
                output,
                16 * 1024,
            )))),
            _ => None,
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> io::Result<Vec<u8>> {
        match self {
            Self::Identity => Ok(bytes.to_vec()),
            Self::Gzip(decoder) => {
                decoder.write_all(bytes)?;
                Ok(std::mem::take(&mut decoder.get_mut().bytes))
            }
            Self::Brotli(decoder) => {
                decoder.write_all(bytes)?;
                Ok(std::mem::take(&mut decoder.get_mut().bytes))
            }
        }
    }
    pub fn finish(&mut self) -> io::Result<Vec<u8>> {
        match self {
            Self::Identity => Ok(Vec::new()),
            Self::Gzip(d) => {
                d.try_finish()?;
                Ok(std::mem::take(&mut d.get_mut().bytes))
            }
            Self::Brotli(d) => {
                d.close()?;
                Ok(std::mem::take(&mut d.get_mut().bytes))
            }
        }
    }
}
