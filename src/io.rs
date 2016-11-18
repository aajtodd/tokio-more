use futures::{Async, Poll};
use bytes::{Buf, BufMut};

use std::io;

pub trait AsyncRead: io::Read {
    /// Pull some bytes from this source into the specified buffer, returning
    /// how many bytes were read.
    ///
    /// If the source is not able to perform the operation due to not having
    /// any bytes to read, `Ok(Async::NotReady)` is returned.
    ///
    /// Aside from the signature, behavior is identical to `std::io::Read`. For
    /// more details, read the `std::io::Read` documentation.
    fn try_read(&mut self, buf: &mut [u8]) -> Poll<usize, io::Error> {
        match self.read(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }

    /// Pull some bytes from this source into the specified `Buf`, returning
    /// how many bytes were read.
    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> io::Result<usize> {
        if !buf.has_remaining_mut() {
            return Ok(0);
        }

        unsafe {
            let i = try!(self.read(buf.bytes_mut()));

            buf.advance_mut(i);
            Ok(i)
        }
    }

    /// Pull some bytes from this source into the specified `Buf`, returning
    /// how many bytes were read.
    fn try_read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match self.read_buf(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }
}

pub trait AsyncWrite: io::Write {
    /// Write a buffer into this object, returning how many bytes were written.
    ///
    /// If the source is not able to perform the operation due to not being
    /// ready, `Ok(Async::NotReady)` is returned.
    ///
    /// Aside from the signature, behavior is identical to `std::io::Write`.
    /// For more details, read the `std::io::Write` documentation.
    fn try_write(&mut self, buf: &[u8]) -> Poll<usize, io::Error> {
        match self.write(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }

    /// Write a `Buf` into this value, returning how many bytes were written.
    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> io::Result<usize> {
        if !buf.has_remaining() {
            return Ok(0);
        }

        let i = try!(self.write(buf.bytes()));
        buf.advance(i);
        Ok(i)
    }

    /// Write a `Buf` into this object, returning how many bytes were written.
    fn try_write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match self.write_buf(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }

    /// Try flushing the underlying IO
    fn try_flush(&mut self) -> Poll<(), io::Error> {
        match self.flush() {
            Ok(()) => Ok(Async::Ready(())),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }
}

impl<T: io::Read> AsyncRead for T {
}

impl<T: io::Write> AsyncWrite for T {
}
