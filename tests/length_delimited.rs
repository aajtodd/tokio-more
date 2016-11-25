extern crate futures;
extern crate tokio_more;
extern crate bytes;
extern crate fixture_io;

use tokio_more::codec::length_delimited::*;
use futures::{Stream, Sink, Future};
use bytes::BytesMut;
use fixture_io::FixtureIo;
use std::io;
use std::time::Duration;

/*
 *
 * ===== Decoder =====
 *
 */

#[test]
pub fn decode_empty_io_yields_nothing() {
    let io = FixtureIo::empty();
    let io = Decoder::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[]));
}

#[test]
pub fn decode_single_frame_one_packet() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00\x00\x09abcdefghi"[..]);

    let io = Decoder::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi"]));
}

#[test]
pub fn decode_single_frame_one_packet_le() {
    let io = FixtureIo::empty()
        .then_read(&b"\x09\x00\x00\x00abcdefghi"[..]);

    let builder = Builder::new().set_byte_order(ByteOrder::LittleEndian);
    let io = builder.decoder(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi"]));
}

#[test]
pub fn decode_single_multi_frame_one_packet() {
    let mut data: Vec<u8> = vec![];
    data.extend_from_slice(b"\x00\x00\x00\x09abcdefghi");
    data.extend_from_slice(b"\x00\x00\x00\x03123");
    data.extend_from_slice(b"\x00\x00\x00\x0bhello world");

    let io = FixtureIo::empty()
        .then_read(data);

    let io = Decoder::default(io);

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

    let io = Decoder::default(io);

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

    let io = Decoder::default(io);

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

    let io = Decoder::default(io);

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

    let io = Decoder::default(io);

    let chunks = collect(io).unwrap();
    assert_eq!(chunks, bytes(&[b"abcdefghi", b"123", b"hello world"]));
}

#[test]
pub fn incomplete_head() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00"[..])
        ;

    let io = Decoder::default(io);

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

    let io = Decoder::default(io);

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

    let io = Decoder::default(io);

    assert!(collect(io).is_err());
}


#[test]
pub fn decode_max_frame_size_exceeded() {
    let io = FixtureIo::empty()
        .then_read(&b"\x00\x00\x00\x09abcdefghi"[..]);

    let io = Builder::new().set_max_frame_length(8).decoder(io);

    assert!(collect(io).is_err());
}

/*
 *
 * ===== Encoder =====
 *
 */

#[test]
pub fn encode_nothing_yields_nothing() {
    let mut io = FixtureIo::empty();
    let rx = io.receiver();
    let io: Encoder<FixtureIo, &'static [u8]> = Encoder::default(io);

    drop(io);
    rx.recv().unwrap();
}

#[test]
pub fn encode_single_frame_one_packet() {
    let mut io = FixtureIo::empty()
        .then_write(&b"\x00\x00\x00\x09abcdefghi"[..]);

    let rx = io.receiver();
    let io = Encoder::default(io);
    let io = io.send(&b"abcdefghi"[..]).wait().unwrap();

    drop(io);
    rx.recv().unwrap();
}

#[test]
pub fn encode_single_frame_one_packet_le() {
    let mut io = FixtureIo::empty()
        .then_write(&b"\x09\x00\x00\x00abcdefghi"[..]);

    let rx = io.receiver();
    let builder = Builder::new().set_byte_order(ByteOrder::LittleEndian);
    let io = builder.encoder(io);
    let io = io.send(&b"abcdefghi"[..]).wait().unwrap();

    drop(io);
    rx.recv().unwrap();
}

#[test]
pub fn encode_single_multi_frame_one_packet() {
    let mut data: Vec<u8> = vec![];
    data.extend_from_slice(b"\x00\x00\x00\x09abcdefghi");
    data.extend_from_slice(b"\x00\x00\x00\x03123");
    data.extend_from_slice(b"\x00\x00\x00\x0bhello world");

    let mut io = FixtureIo::empty()
        .then_write(data);

    let rx = io.receiver();
    let io = Encoder::default(io);

    let io = io.send(&b"abcdefghi"[..]).wait().unwrap();
    let io = io.send(&b"123"[..]).wait().unwrap();
    let io = io.send(&b"hello world"[..]).wait().unwrap();

    drop(io);
    rx.recv().unwrap();
}

#[test]
pub fn encode_max_frame_size_exceeded() {
    let mut io = FixtureIo::empty()
        .then_write(&b"\x00\x00\x00\x09abcdefghi"[..]);

    let rx = io.receiver();
    let io = Builder::new().set_max_frame_length(8).encoder(io);
    let io = io.send(&b"abcdefghi"[..]).wait();
    assert!(io.is_err());
}

/*
 *
 * ===== Util =====
 *
 */

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
