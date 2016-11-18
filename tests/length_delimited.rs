extern crate futures;
extern crate tokio_more;
extern crate bytes;
extern crate fixture_io;

use tokio_more::codec::LengthDelimited;
use futures::{Stream};
use bytes::BytesMut;
use fixture_io::FixtureIo;
use std::io;
use std::time::Duration;

#[test]
pub fn empty_io_yields_nothing() {
    let io = FixtureIo::empty();
    let io = LengthDelimited::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[]));
}

#[test]
pub fn single_frame_one_packet() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00\x00\x09abcdefghi"[..]);

    let io = LengthDelimited::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi"]));
}

#[test]
pub fn single_multi_frame_one_packet() {
    let mut data: Vec<u8> = vec![];
    data.extend_from_slice(b"\x00\x00\x00\x09abcdefghi");
    data.extend_from_slice(b"\x00\x00\x00\x03123");
    data.extend_from_slice(b"\x00\x00\x00\x0bhello world");

    let io = FixtureIo::empty()
        .then_read(data);

    let io = LengthDelimited::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi", b"123", b"hello world"]));
}

#[test]
pub fn single_frame_multi_packet() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00"[..])
        .then_read(&b"\x00\x09abc"[..])
        .then_read(&b"defghi"[..])
        ;

    let io = LengthDelimited::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi"]));
}

#[test]
pub fn multi_frame_multi_packet() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00"[..])
        .then_read(&b"\x00\x09abc"[..])
        .then_read(&b"defghi"[..])
        .then_read(&b"\x00\x00\x00\x0312"[..])
        .then_read(&b"3\x00\x00\x00\x0bhello world"[..])
        ;

    let io = LengthDelimited::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi", b"123", b"hello world"]));
}

#[test]
pub fn single_frame_multi_packet_wait() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00"[..])
        .then_wait(ms(50))
        .then_read(&b"\x00\x09abc"[..])
        .then_wait(ms(50))
        .then_read(&b"defghi"[..])
        .then_wait(ms(50))
        ;

    let io = LengthDelimited::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi"]));
}

#[test]
pub fn multi_frame_multi_packet_wait() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00"[..])
        .then_wait(ms(50))
        .then_read(&b"\x00\x09abc"[..])
        .then_wait(ms(50))
        .then_read(&b"defghi"[..])
        .then_wait(ms(50))
        .then_read(&b"\x00\x00\x00\x0312"[..])
        .then_wait(ms(50))
        .then_read(&b"3\x00\x00\x00\x0bhello world"[..])
        .then_wait(ms(50))
        ;

    let io = LengthDelimited::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi", b"123", b"hello world"]));
}

#[test]
pub fn incomplete_head() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00"[..])
        ;

    let io = LengthDelimited::default(io);

    assert!(collect(io).is_err());
}

#[test]
pub fn incomplete_head_multi() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00"[..])
        .then_wait(ms(50))
        .then_read(&b"\x00"[..])
        .then_wait(ms(50))
        ;

    let io = LengthDelimited::default(io);

    assert!(collect(io).is_err());
}

#[test]
pub fn incomplete_payload() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00\x00\x09ab"[..])
        .then_wait(ms(50))
        .then_read(&b"cd"[..])
        .then_wait(ms(50))
        ;

    let io = LengthDelimited::default(io);

    assert!(collect(io).is_err());
}

fn collect<T>(io: T) -> io::Result<Vec<T::Item>>
    where T: Stream<Item = BytesMut, Error = io::Error>
{
    let mut ret = vec![];

    for v in io.wait() {
        ret.push(try!(v));
    }

    Ok(ret)
}

fn bytes(elems: &[&[u8]]) -> Vec<BytesMut> {
    elems.iter()
        .map(|&e| e.into())
        .collect()
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
