use io::{AsyncRead, AsyncWrite};
use bytes::{Buf, BufMut, BytesMut, ByteBuf};
use futures::{Async, Poll, Sink, Stream, StartSend};
use byteorder::{BigEndian, LittleEndian};

use std::io::{self, Read, Write};

pub struct LengthDelimited<T> {
    // I/O type
    inner: T,

    // Configuration values
    builder: Builder,

    // Buffer
    buf: ByteBuf,

    // Read state
    state: State,
}

pub struct Builder {
    // Maximum frame length
    max_frame_len: usize,

    // Number of bytes representing the field length
    length_field_len: usize,

    // Length field byte order (little or big endian)
    length_field_order: ByteOrder,
}

/// An enumeration of valid byte orders
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum ByteOrder {
    /// Big-endian byte order.
    BigEndian,

    /// Little-endian byte order.
    LittleEndian,
}

#[derive(Debug, Clone, Copy)]
enum State {
    Head,
    Data(usize),
}

impl<T> LengthDelimited<T> {
    pub fn default(io: T) -> LengthDelimited<T> {
        LengthDelimited::builder().build(io)
    }

    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl LengthDelimited<()> {
    pub fn builder() -> Builder {
        Builder {
            // Default max frame length of 8MB
            max_frame_len: 8 * 1_024 * 1_024,

            // Default byte length of 4
            length_field_len: 4,

            // Default to reading the length field in network (big) endian.
            length_field_order: ByteOrder::BigEndian,
        }
    }
}

impl<T: AsyncRead> LengthDelimited<T> {
    fn read_head(&mut self) -> Poll<Option<usize>, io::Error> {
        let field_len = self.builder.length_field_len;

        loop {
            if self.buf.len() >= field_len {
                // Enough data has been buffered to process the head
                let n = match self.builder.length_field_order {
                    ByteOrder::BigEndian => {
                        self.buf.get_uint::<BigEndian>(field_len)
                    }
                    ByteOrder::LittleEndian => {
                        self.buf.get_uint::<LittleEndian>(field_len)
                    }
                };

                if n > self.builder.max_frame_len as u64 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "frame size too big"));
                }

                // The check above ensures there is no overflow
                let n = n as usize;

                // TODO: Add a config setting to not consume the head
                self.buf.drain_to(field_len);

                // Ensure that the buffer has enough space to read the incoming
                // payload
                self.buf.reserve(n);

                return Ok(Async::Ready(Some(n)));
            }

            // Ensure the buffer has enough space
            let rem = field_len - self.buf.len();
            self.buf.reserve(rem);

            // Try reading the rest of the head
            let read = try_ready!(self.inner.try_read_buf(&mut self.buf));

            // If 0 bytes have been read, then the upstream has been shutdown.
            if read == 0 {
                if self.buf.is_empty() {
                    return Ok(Async::Ready(None));
                } else {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof"));
                }
            }
        }
    }

    fn read_data(&mut self, n: usize) -> Poll<Option<BytesMut>, io::Error> {
        // At this point, the buffer has already had the required capacity
        // reserved. All there is to do is read.
        loop {
            if self.buf.len() >= n {
                let ret = self.buf.drain_to(n);
                return Ok(Async::Ready(Some(ret)));
            }

            let read = try_ready!(self.inner.try_read_buf(&mut self.buf));

            // Same as `read_head` except that the upstream should never
            // shutdown at this point, thus making a shutdown always an error.
            if read == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof"));
            }
        }
    }
}

impl<T: AsyncRead> Stream for LengthDelimited<T> {
    type Item = BytesMut;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<BytesMut>, io::Error> {
        loop {
            match self.state {
                State::Head => {
                    match try_ready!(self.read_head()) {
                        Some(n) => self.state = State::Data(n),
                        None => return Ok(Async::Ready(None)),
                    }
                }
                State::Data(n) => {
                    let data = try_ready!(self.read_data(n));
                    self.state = State::Head;
                    return Ok(Async::Ready(data));
                }
            }
        }
    }
}

impl<T: Write> Write for LengthDelimited<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<T: Sink> Sink for LengthDelimited<T> {
    type SinkItem = T::SinkItem;
    type SinkError = T::SinkError;

    fn start_send(&mut self, item: T::SinkItem)
        -> StartSend<T::SinkItem, T::SinkError>
    {
        self.inner.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), T::SinkError> {
        self.inner.poll_complete()
    }
}

/*
 *
 * ===== impl Builder
 *
 */

impl Builder {
    /// Sets the max frame length
    pub fn set_max_frame_length(mut self, val: usize) -> Self {
        self.max_frame_len = val;
        self
    }

    /// Sets the number of bytes used to represent the length field
    pub fn set_length_field_length(mut self, val: usize) -> Self {
        assert!(val > 0 && val <= 8, "invalid length field length");
        self.length_field_len = val;
        self
    }

    /// Build the length delimted decoder
    pub fn build<T>(self, io: T) -> LengthDelimited<T> {
        LengthDelimited {
            inner: io,
            builder: self,
            buf: ByteBuf::new(),
            state: State::Head,
        }
    }
}
