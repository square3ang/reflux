//! One zstd stream per Gateway connection (`compress=zstd-stream&stream=1`): every server frame
//! goes through one decompressor in order, every client message through one compressor that is
//! flushed per message.

use std::io;

use serde_json::Value;
use zstd::stream::raw::{Decoder, Encoder, InBuffer, Operation, OutBuffer};

/// Compression level of the reference client.
const LEVEL: i32 = 3;

pub(crate) struct ZstdStream {
    dec: Decoder<'static>,
    enc: Encoder<'static>,
    /// Decompressed bytes not yet parsed into a complete JSON value.
    pending: Vec<u8>,
    buf: Box<[u8]>,
}

impl ZstdStream {
    pub fn new() -> io::Result<Self> {
        Ok(Self { dec: Decoder::new()?, enc: Encoder::new(LEVEL)?, pending: Vec::new(), buf: vec![0; 64 * 1024].into() })
    }

    /// Compresses one message and flushes, so the peer can decode it without later data.
    pub fn compress(&mut self, msg: &[u8]) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut input = InBuffer::around(msg);
        while input.pos() < msg.len() {
            let mut o = OutBuffer::around(&mut self.buf[..]);
            self.enc.run(&mut input, &mut o)?;
            out.extend_from_slice(o.as_slice());
        }
        loop {
            let mut o = OutBuffer::around(&mut self.buf[..]);
            let remaining = self.enc.flush(&mut o)?;
            out.extend_from_slice(o.as_slice());
            if remaining == 0 {
                return Ok(out);
            }
        }
    }

    /// Feeds one frame and returns every JSON value it completes (a value may span frames).
    pub fn decompress(&mut self, frame: &[u8]) -> io::Result<Vec<Value>> {
        let mut input = InBuffer::around(frame);
        loop {
            let mut o = OutBuffer::around(&mut self.buf[..]);
            self.dec.run(&mut input, &mut o)?;
            let (n, full) = (o.pos(), o.pos() == o.capacity());
            self.pending.extend_from_slice(&self.buf[..n]);
            if input.pos() == frame.len() && !full {
                break;
            }
        }
        let mut values = Vec::new();
        let mut stream = serde_json::Deserializer::from_slice(&self.pending).into_iter::<Value>();
        for v in stream.by_ref() {
            match v {
                Ok(v) => values.push(v),
                Err(e) if e.is_eof() => break,
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
            }
        }
        let used = stream.byte_offset();
        self.pending.drain(..used);
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_split_across_messages() {
        let msgs = [r#"{"op":10,"d":{"heartbeat_interval":41250}}"#, r#"{"op":0,"t":"X","s":1,"d":{"a":"bcdef"}}"#];
        let mut server = ZstdStream::new().unwrap();
        let wire: Vec<u8> = msgs.iter().flat_map(|m| server.compress(m.as_bytes()).unwrap()).collect();

        // Every split point, including splits inside zstd blocks and between both messages.
        for cut in 1..wire.len() {
            let mut client = ZstdStream::new().unwrap();
            let mut got = client.decompress(&wire[..cut]).unwrap();
            got.extend(client.decompress(&wire[cut..]).unwrap());
            let want: Vec<Value> = msgs.iter().map(|m| serde_json::from_str(m).unwrap()).collect();
            assert_eq!(got, want, "cut at {cut}");
            assert!(client.pending.is_empty());
        }
    }

    #[test]
    fn one_message_per_frame_and_large_payload() {
        let mut server = ZstdStream::new().unwrap();
        let mut client = ZstdStream::new().unwrap();
        let big = format!(r#"{{"op":0,"d":"{}"}}"#, "x".repeat(300_000));
        for m in [r#"{"op":11}"#, big.as_str(), r#"{"op":11}"#] {
            let frame = server.compress(m.as_bytes()).unwrap();
            assert_eq!(client.decompress(&frame).unwrap(), vec![serde_json::from_str::<Value>(m).unwrap()]);
        }
        assert!(client.decompress(b"not zstd").is_err());
    }
}
