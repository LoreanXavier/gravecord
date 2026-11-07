//! `compress=zstd-stream`: one zstd stream for the life of a socket, one flushed block per WebSocket message.

use std::io;
use zstd::stream::raw::{Decoder, InBuffer, Operation, OutBuffer};

/// Frames larger than this are refused rather than buffered.
const MAX_MESSAGE: usize = 256 << 20;

pub struct Inflater {
    decoder: Decoder<'static>,
    buffer: Vec<u8>,
}

impl Inflater {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            decoder: Decoder::new()?,
            buffer: vec![0; 1 << 16],
        })
    }

    pub fn feed(&mut self, compressed: &[u8]) -> io::Result<Vec<u8>> {
        let mut input = InBuffer::around(compressed);
        let mut message = Vec::new();
        loop {
            let mut output = OutBuffer::around(self.buffer.as_mut_slice());
            self.decoder.run(&mut input, &mut output)?;
            let written = output.pos();
            let filled = written == self.buffer.len();
            message.extend_from_slice(&self.buffer[..written]);
            if message.len() > MAX_MESSAGE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "gateway message too large",
                ));
            }
            // A full buffer can hide more output even after the input has run out.
            if input.pos() == compressed.len() && !filled {
                return Ok(message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zstd::stream::raw::Encoder;

    fn flushed_block(encoder: &mut Encoder<'static>, data: &[u8]) -> Vec<u8> {
        let mut scratch = vec![0; data.len() + (1 << 17)];
        let mut input = InBuffer::around(data);
        let mut output = OutBuffer::around(scratch.as_mut_slice());
        while input.pos() < data.len() {
            encoder.run(&mut input, &mut output).unwrap();
        }
        while encoder.flush(&mut output).unwrap() != 0 {}
        let written = output.pos();
        scratch.truncate(written);
        scratch
    }

    #[test]
    fn decodes_each_message_of_one_continuous_stream() {
        let mut encoder = Encoder::new(3).unwrap();
        let mut inflater = Inflater::new().unwrap();
        for text in [
            "first message",
            "second message, which reuses the window",
            "third",
        ] {
            let block = flushed_block(&mut encoder, text.as_bytes());
            assert_eq!(inflater.feed(&block).unwrap(), text.as_bytes());
        }
    }

    #[test]
    fn decodes_a_message_larger_than_the_buffer() {
        let data: Vec<u8> = (0..400_000u32)
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let mut encoder = Encoder::new(3).unwrap();
        let block = flushed_block(&mut encoder, &data);
        assert_eq!(Inflater::new().unwrap().feed(&block).unwrap(), data);
    }

    #[test]
    fn refuses_garbage() {
        assert!(
            Inflater::new()
                .unwrap()
                .feed(&[1, 2, 3, 4, 5, 6, 7, 8])
                .is_err()
        );
    }
}
