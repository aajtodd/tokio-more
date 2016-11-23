use io::{AsyncRead, AsyncWrite};
use bytes::{Buf, IntoBuf, BufMut, BytesMut, ByteBuf, SliceBuf};
use futures::{Async, AsyncSink, Poll, Sink, Stream, StartSend};
use byteorder::{BigEndian, LittleEndian};

use std::{cmp, mem};
use std::io::{self, Read, Write};

/// A decoder that splits the bytes read into `BytesMut` values according to
/// the value of the length field in the frame header.
///
/// This decoder may be used for protocols that delimit the frame payloads
/// using an integer denoting the payload length. The decoder reads the frame
/// header containing the length field `n`, then reads the next `n` bytes and
/// yields them as a `BytesMut`.
pub struct Decoder<T> {
    // I/O type
    inner: T,

    // Configuration values
    builder: Builder,

    // Buffer
    buf: ByteBuf,

    // Read state
    state: ReadState,
}

pub struct Encoder<T, B: IntoBuf> {
    // I/O type
    inner: T,

    // Configuration values
    builder: Builder,

    // Write state
    state: WriteState<B::Buf>,
}

pub struct Builder {
    // Maximum frame length
    max_frame_len: usize,

    // Number of bytes representing the field length
    length_field_len: usize,

    // Number of bytes in the header before the length field
    length_field_offset: usize,

    // Adjust the length specified in the header field by this amount
    length_adjustment: isize,

    // Total number of bytes to skip before reading the payload, if not set,
    // `length_field_len + length_field_offset`
    num_skip: Option<usize>,

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
enum ReadState {
    Head,
    Data(usize),
}

enum WriteState<B> {
    Ready,
    Head { head: SliceBuf<[u8; 8]>, data: B },
    Data(B),
}

/*
 *
 * ===== impl Decoder =====
 *
 */

impl<T> Decoder<T> {
    pub fn default(io: T) -> Decoder<T> {
        Builder::new().decoder(io)
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

impl<T: AsyncRead> Decoder<T> {
    fn read_head(&mut self) -> Poll<Option<usize>, io::Error> {
        let head_len = self.builder.num_head_bytes();
        let field_len = self.builder.length_field_len;

        loop {
            if self.buf.len() >= head_len {
                // Skip the required bytes
                self.buf.advance(self.builder.length_field_offset);

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

                // Adjust `n` with bounds checking
                let n = if self.builder.length_adjustment < 0 {
                    n.checked_sub(-self.builder.length_adjustment as usize)
                } else {
                    n.checked_add(self.builder.length_adjustment as usize)
                };

                // Error handling
                let n = match n {
                    Some(n) => n,
                    None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "provided length would overflow after adjustment")),
                };

                // TODO: Add a config setting to not consume the head
                self.buf.drain_to(self.builder.num_skip());

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

impl<T: AsyncRead> Stream for Decoder<T> {
    type Item = BytesMut;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<BytesMut>, io::Error> {
        loop {
            match self.state {
                ReadState::Head => {
                    match try_ready!(self.read_head()) {
                        Some(n) => self.state = ReadState::Data(n),
                        None => return Ok(Async::Ready(None)),
                    }
                }
                ReadState::Data(n) => {
                    let data = try_ready!(self.read_data(n));
                    self.state = ReadState::Head;
                    return Ok(Async::Ready(data));
                }
            }
        }
    }
}

impl<T: Write> Write for Decoder<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<T: Sink> Sink for Decoder<T> {
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
 * ===== impl Encoder =====
 *
 */

impl<T, B: IntoBuf> Encoder<T, B> {
    pub fn default(io: T) -> Encoder<T, B> {
        Builder::new().encoder(io)
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

impl<T: AsyncWrite, B: IntoBuf> Encoder<T, B> {
    fn set_head(&mut self, buf: B::Buf) -> io::Result<()> {
        let mut head = SliceBuf::new([0; 8]);
        let n = buf.remaining();

        if n > self.builder.max_frame_len {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too big"));
        }

        if self.builder.length_field_order == ByteOrder::BigEndian {
            head.put_uint::<BigEndian>(n as u64, self.builder.length_field_len);
        } else {
            head.put_uint::<LittleEndian>(n as u64, self.builder.length_field_len);
        }

        self.state = WriteState::Head { head: head, data: buf };
        Ok(())
    }

    // Write a frame head. This function will be called as part of
    // `FramedIo::flush`.
    fn write_head(&mut self) -> Poll<(), io::Error> {
        // Loop as long as the upstream is ready
        loop {
            // Get a reference to the buffer.
            let buf = match self.state {
                WriteState::Head { ref mut head, .. } => head,
                _ => unreachable!(),
            };

            // If there is no more data to write, then the frame head has been
            // fully written, so return Ok.
            if !buf.has_remaining() {
                return Ok(Async::Ready(()));
            };

            // Write the data to the upstream. In the write case, 0 does not
            // mean that the upstream has shutdown, so there is no need to
            // check.
            try_ready!(self.inner.try_write_buf(buf));
        }
    }

    // Write a frame payload. This function will be called as part of
    // `FramedIo::flush`. This fn is very similar to `write_head`
    fn write_data(&mut self) -> Poll<(), io::Error> {
        // Loop as long as the upstream is ready
        loop {
            // Get a reference to the buffer.
            let buf = match self.state {
                WriteState::Data(ref mut buf) => buf,
                _ => unreachable!(),
            };

            // If there is no more data to write, then the frame payload has been
            // fully written, so return Okl
            if !buf.has_remaining() {
                return Ok(Async::Ready(()));
            };

            // Write the data to the upstream. In the write case, 0 does not
            // mean that the upstream has shutdown, so there is no need to
            // check.
            try_ready!(self.inner.try_write_buf(buf));
        }
    }
}

impl<T: AsyncWrite, B: IntoBuf> Sink for Encoder<T, B> {
    type SinkItem = B;
    type SinkError = io::Error;

    fn start_send(&mut self, item: B)
        -> StartSend<B, io::Error>
    {
        if !try!(self.poll_complete()).is_ready() {
            return Ok(AsyncSink::NotReady(item));
        }

        // Convert the value to a buffer
        try!(self.set_head(item.into_buf()));

        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        loop {
            match self.state {
                // If in the ready state, then all data has been fully flushed
                // and there is nothing more to do
                WriteState::Ready => return Ok(Async::Ready(())),

                // Currently writing the frame head
                WriteState::Head { .. } => {
                    // Write the frame head, returning if `write_head` returns
                    // an error or `NotReady`
                    try_ready!(self.write_head());

                    // The head has been fully written to the upstream, transition to
                    // writing the payload
                    match mem::replace(&mut self.state, WriteState::Ready) {
                        WriteState::Head { data, .. } => {
                            self.state = WriteState::Data(data);
                        }
                        _ => unreachable!(),
                    }
                }

                // Currently writing the frame payload
                WriteState::Data(..) => {
                    // Write the frame payload, returning if `write_data` returns
                    // an error or `NotReady`
                    try_ready!(self.write_data());

                    // The payload has been fully written to the upstream,
                    // transition to ready.
                    self.state = WriteState::Ready;
                }
            }
        }
    }
}

impl<T: Read, B: IntoBuf> Read for Encoder<T, B> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<T: Stream, B: IntoBuf> Stream for Encoder<T, B> {
    type Item = T::Item;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<T::Item>, T::Error> {
        self.inner.poll()
    }
}

/*
 *
 * ===== impl Builder =====
 *
 */

impl Builder {
    pub fn new() -> Builder {
        Builder {
            // Default max frame length of 8MB
            max_frame_len: 8 * 1_024 * 1_024,

            // Default byte length of 4
            length_field_len: 4,

            // Default to the header field being at the start of the header.
            length_field_offset: 0,

            length_adjustment: 0,

            // Total number of bytes to skip before reading the payload, if not set,
            // `length_field_len + length_field_offset`
            num_skip: None,

            // Default to reading the length field in network (big) endian.
            length_field_order: ByteOrder::BigEndian,
        }
    }

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

    /// Sets the number of bytes in the header before the length field
    pub fn set_length_field_offset(mut self, val: usize) -> Self {
        self.length_field_offset = val;
        self
    }

    /// Delta between the payload length specified in the header and the real
    /// payload length
    pub fn set_length_adjustment(mut self, val: isize) -> Self {
        self.length_adjustment = val;
        self
    }

    /// Sets the number of bytes to skip before reading the payload
    ///
    /// Defaults to `length_field_len + length_field_offset`
    pub fn set_num_skip(mut self, val: usize) -> Self {
        self.num_skip = Some(val);
        self
    }

    /// Build the length delimted decoder
    pub fn decoder<T>(self, io: T) -> Decoder<T> {
        Decoder {
            inner: io,
            builder: self,
            buf: ByteBuf::new(),
            state: ReadState::Head,
        }
    }

    pub fn encoder<T, B: IntoBuf>(self, io: T) -> Encoder<T, B> {
        Encoder {
            inner: io,
            builder: self,
            state: WriteState::Ready,
        }
    }

    /// Number of header bytes to read
    fn num_head_bytes(&self) -> usize {
        let num = self.length_field_offset + self.length_field_len;
        cmp::max(num, self.num_skip.unwrap_or(0))
    }

    fn num_skip(&self) -> usize {
        self.num_skip.unwrap_or(self.length_field_offset + self.length_field_len)
    }
}
