use crate::{ErrorKind, RefscapeError};
use std::{borrow::Borrow, fmt, ops::Deref};

macro_rules! string_id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, RefscapeError> {
                let value = value.into();
                if value.is_empty() { return Err(RefscapeError::new(ErrorKind::InvalidData, concat!(stringify!($name), " must be nonempty"))); }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str { &self.0 }
            pub fn into_string(self) -> String { self.0 }
        }
        impl Deref for $name { type Target = str; fn deref(&self) -> &str { &self.0 } }
        impl AsRef<str> for $name { fn as_ref(&self) -> &str { &self.0 } }
        impl Borrow<str> for $name { fn borrow(&self) -> &str { &self.0 } }
        impl fmt::Display for $name { fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result { self.0.fmt(f) } }
        impl From<$name> for String { fn from(id:$name)->Self { id.0 } }
        impl From<String> for $name { fn from(value:String)->Self { Self(value) } }
        impl From<&str> for $name { fn from(value:&str)->Self { Self(value.into()) } }
        impl PartialEq<String> for $name { fn eq(&self, other:&String)->bool { self.0 == *other } }
        impl PartialEq<$name> for String { fn eq(&self, other:&$name)->bool { *self == other.0 } }
        impl PartialEq<str> for $name { fn eq(&self, other:&str)->bool { self.0 == other } }
        impl PartialEq<&str> for $name { fn eq(&self, other:&&str)->bool { self.0 == *other } }
    )+};
}
string_id!(CardId, EdgeId, SnapshotId);

macro_rules! counter_id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u64);
        impl $name { pub const fn new(value:u64)->Self { Self(value) } pub const fn get(self)->u64 { self.0 } }
        impl fmt::Display for $name { fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result { self.0.fmt(f) } }
        impl From<u64> for $name { fn from(value:u64)->Self { Self(value) } }
    )+};
}
counter_id!(ProjectEpoch, JobId, FoldId, SourceRevision, FoldRevision);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utf16Column(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceLineNumber(pub u32);
