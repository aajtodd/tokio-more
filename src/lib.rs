extern crate tokio_core;
extern crate bytes;
extern crate byteorder;

#[macro_use]
extern crate futures;

pub mod codec;

mod io;

pub use io::{AsyncRead, AsyncWrite};
