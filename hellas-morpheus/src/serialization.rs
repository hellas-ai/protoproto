use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Debug)]
pub struct ArkSerialize<T>(pub T);

impl<T> redb::Value for ArkSerialize<T>
where
    T: Debug + CanonicalDeserialize + CanonicalSerialize,
{
    type SelfType<'a>
        = T
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        T::deserialize_compressed(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a,
        Self: 'b,
    {
        let mut writer = Vec::new();
        T::serialize_compressed(value, &mut writer).unwrap();
        writer
    }

    fn type_name() -> redb::TypeName {
        redb::TypeName::new(&format!("ArkSerialize<{}>", std::any::type_name::<T>()))
    }
}

impl<T> redb::Key for ArkSerialize<T>
where
    T: Debug + CanonicalDeserialize + CanonicalSerialize + Ord,
{
    fn compare(data1: &[u8], data2: &[u8]) -> std::cmp::Ordering {
        use redb::Value;
        Self::from_bytes(data1).cmp(&Self::from_bytes(data2))
    }
}

#[derive(Debug)]
pub struct Postcard<T>(pub T);

impl<T> redb::Value for Postcard<T>
where
    T: Debug + Serialize + for<'a> Deserialize<'a>,
{
    type SelfType<'a>
        = T
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        postcard::from_bytes(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a,
        Self: 'b,
    {
        postcard::to_stdvec(value).unwrap()
    }

    fn type_name() -> redb::TypeName {
        redb::TypeName::new(&format!("Postcard<{}>", std::any::type_name::<T>()))
    }
}

impl<T> redb::Key for Postcard<T>
where
    T: Debug + Serialize + for<'a> Deserialize<'a> + Ord,
{
    fn compare(data1: &[u8], data2: &[u8]) -> std::cmp::Ordering {
        use redb::Value;
        Self::from_bytes(data1).cmp(&Self::from_bytes(data2))
    }
}