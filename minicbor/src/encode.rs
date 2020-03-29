//! Traits and types for encoding CBOR.
//!
//! This module defines the trait [`Encode`] and the actual [`Encoder`].
//! It also defines a [`Write`] trait to store the encoded bytes.

mod encoder;
mod error;
mod iter;
pub mod write;

pub use encoder::Encoder;
pub use error::Error;
pub use iter::{Iter, ExactSizeIter};
pub use write::Write;

/// A type that can be encoded to CBOR.
///
/// If this type's CBOR encoding is meant to be decoded by `Decode` impls
/// derived with [`minicbor_derive`] *it is advisable to only produce a
/// single CBOR data item*. Tagging, maps or arrays can and should be used
/// for multiple values.
pub trait Encode {
    /// Encode a value of this type using the given `Encoder`.
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>>;
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        (**self).encode(e)
    }
}

impl<T: Encode + ?Sized> Encode for &mut T {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        (**self).encode(e)
    }
}

#[cfg(feature = "std")]
impl<T: Encode + ?Sized> Encode for Box<T> {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        (**self).encode(e)
    }
}

impl Encode for [u8] {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.bytes(self)?.ok()
    }
}

impl Encode for str {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.str(self)?.ok()
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        if let Some(x) = self {
            x.encode(e)?;
        } else {
            e.null()?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl Encode for String {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.str(self)?.ok()
    }
}

#[cfg(feature = "std")]
impl<T> Encode for std::borrow::Cow<'_, T>
where
    T: Encode + Clone
{
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        self.as_ref().encode(e)
    }
}


#[cfg(feature = "std")]
impl<K, V> Encode for std::collections::HashMap<K, V>
where
    K: Encode + Eq + std::hash::Hash,
    V: Encode
{
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.map(self.len())?;
        for (k, v) in self {
            k.encode(e)?;
            v.encode(e)?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl<K, V> Encode for std::collections::BTreeMap<K, V>
where
    K: Encode + Eq + Ord,
    V: Encode
{
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.map(self.len())?;
        for (k, v) in self {
            k.encode(e)?;
            v.encode(e)?;
        }
        Ok(())
    }
}

impl<T> Encode for core::marker::PhantomData<T> {
    fn encode<W: Write>(&self, _: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        Ok(())
    }
}

#[cfg(target_pointer_width = "32")]
impl Encode for usize {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.u32(*self as u32)?.ok()
    }
}

#[cfg(target_pointer_width = "64")]
impl Encode for usize {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.u64(*self as u64)?.ok()
    }
}

#[cfg(target_pointer_width = "32")]
impl Encode for isize {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.i32(*self as i32)?.ok()
    }
}

#[cfg(target_pointer_width = "64")]
impl Encode for isize {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.i64(*self as i64)?.ok()
    }
}

macro_rules! encode_basic {
    ($($t:ident)*) => {
        $(
            impl $crate::encode::Encode for $t {
                fn encode<W>(&self, e: &mut $crate::encode::Encoder<W>) -> Result<(), Error<W::Error>>
                where
                    W: $crate::encode::Write
                {
                    e.$t(*self)?;
                    Ok(())
                }
            }
        )*
    }
}

encode_basic!(u8 i8 u16 i16 u32 i32 u64 i64 bool f32 f64 char);

macro_rules! encode_nonzero {
    ($($t:ty)*) => {
        $(
            impl $crate::encode::Encode for $t {
                fn encode<W>(&self, e: &mut $crate::encode::Encoder<W>) -> Result<(), Error<W::Error>>
                where
                    W: $crate::encode::Write
                {
                    self.get().encode(e)
                }
            }
        )*
    }
}

encode_nonzero! {
    core::num::NonZeroU8
    core::num::NonZeroU16
    core::num::NonZeroU32
    core::num::NonZeroU64
    core::num::NonZeroI8
    core::num::NonZeroI16
    core::num::NonZeroI32
    core::num::NonZeroI64
}

#[cfg(feature = "std")]
macro_rules! encode_sequential {
    ($($t:ty)*) => {
        $(
            impl<T> $crate::encode::Encode for $t
            where
                T: $crate::encode::Encode
            {
                fn encode<W>(&self, e: &mut $crate::encode::Encoder<W>) -> Result<(), Error<W::Error>>
                where
                    W: $crate::encode::Write
                {
                    e.array(self.len())?;
                    for x in self {
                        x.encode(e)?
                    }
                    Ok(())
                }
            }
        )*
    }
}

#[cfg(feature = "std")]
encode_sequential! {
    Vec<T>
    std::collections::VecDeque<T>
    std::collections::LinkedList<T>
    std::collections::BinaryHeap<T>
    std::collections::HashSet<T>
    std::collections::BTreeSet<T>
}

macro_rules! encode_arrays {
    ($($n:expr)*) => {
        $(
            impl<T> $crate::encode::Encode for [T; $n]
            where
                T: $crate::encode::Encode
            {
                fn encode<W>(&self, e: &mut $crate::encode::Encoder<W>) -> Result<(), Error<W::Error>>
                where
                    W: $crate::encode::Write
                {
                    e.array($n)?;
                    for x in self {
                        x.encode(e)?
                    }
                    Ok(())
                }
            }
        )*
    }
}

encode_arrays!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16);

#[cfg(feature = "smallvec")]
macro_rules! encode_smallvecs {
    ($($n:expr)*) => {
        $(
            impl<T> $crate::encode::Encode for smallvec::SmallVec::<[T; $n]>
            where
                T: $crate::encode::Encode
            {
                fn encode<W>(&self, e: &mut $crate::encode::Encoder<W>) -> Result<(), Error<W::Error>>
                where
                    W: $crate::encode::Write
                {
                    e.array(self.len())?;
                    for x in self {
                        x.encode(e)?
                    }
                    Ok(())
                }
            }
        )*
    }
}

#[cfg(feature = "smallvec")]
encode_smallvecs!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16);

impl Encode for core::time::Duration {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.map(2)?
            .u8(0)?.encode(self.as_secs())?
            .u8(1)?.encode(self.subsec_nanos())?
            .ok()
    }
}

#[cfg(feature = "std")]
impl Encode for std::net::IpAddr {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.array(2)?;
        match self {
            std::net::IpAddr::V4(a) => e.u32(0)?.encode(a)?.ok(),
            std::net::IpAddr::V6(a) => e.u32(1)?.encode(a)?.ok()
        }
    }
}

#[cfg(feature = "std")]
impl Encode for std::net::Ipv4Addr {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        self.octets().encode(e)
    }
}

#[cfg(feature = "std")]
impl Encode for std::net::Ipv6Addr {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        self.octets().encode(e)
    }
}

#[cfg(feature = "std")]
impl Encode for std::net::SocketAddr {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.array(2)?;
        match self {
            std::net::SocketAddr::V4(a) => e.u32(0)?.encode(a)?.ok(),
            std::net::SocketAddr::V6(a) => e.u32(1)?.encode(a)?.ok()
        }
    }
}

#[cfg(feature = "std")]
impl Encode for std::net::SocketAddrV4 {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.map(2)?
            .u32(0)?.encode(self.ip())?
            .u32(1)?.encode(self.port())?
            .ok()
    }
}

#[cfg(feature = "std")]
impl Encode for std::net::SocketAddrV6 {
    fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), Error<W::Error>> {
        e.map(2)?
            .u32(0)?.encode(self.ip())?
            .u32(1)?.encode(self.port())?
            .ok()
    }
}

