use std::{collections::HashMap, convert::TryInto, hash::Hash};

use super::{
    context::{
        PacketDeserializeError, PacketDeserializeResult, PacketDeserializerContext,
        PacketSerializeResult, PacketSerializerContext,
    },
    packet::PacketCodec,
};

macro_rules! gen_numeric_codec {
    (@single $gen_type:ty) => {
        impl PacketCodec for $gen_type {
            fn serialize<'buf>(
                &self,
                ctx: &mut PacketSerializerContext<'buf>,
            ) -> PacketSerializeResult<()> {
                ctx.write_byte_sequence(self.to_be_bytes())?;
                Ok(())
            }

            fn deserialize<'buf>(
                ctx: &mut PacketDeserializerContext<'buf>,
            ) -> PacketDeserializeResult<Self> {
                let bytes = ctx.parse_byte_sequence(std::mem::size_of::<$gen_type>())?;
                Ok(<$gen_type>::from_be_bytes(bytes.try_into().unwrap()))
            }
        }
    };

    ($($gen_type:ty,)*) => {
        $(gen_numeric_codec! { @single $gen_type })*
    };
}

gen_numeric_codec! {
    u8, u16, u32, u64, u128,
    i8, i16, i32, i64, i128,
    f32, f64,
}

impl PacketCodec for String {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize(&(self.len() as u32))?;
        ctx.write_byte_sequence(self.as_bytes().iter().copied())?;
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        let length = ctx.deserialize::<u32>()?;
        let bytes = ctx.parse_byte_sequence(length as usize)?;
        Ok(std::str::from_utf8(bytes)?.into())
    }
}

impl<T: PacketCodec> PacketCodec for Vec<T> {
    fn serialize(&self, ctx: &mut PacketSerializerContext) -> PacketSerializeResult<()> {
        ctx.serialize(&(self.len() as u32))?;
        for item in self.iter() {
            ctx.serialize(item)?;
        }
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        let length = ctx.deserialize::<u32>()?;
        let mut values = Vec::new();
        for _ in 0..length {
            values.push(ctx.deserialize::<T>()?);
        }
        Ok(values)
    }
}

impl<K, V> PacketCodec for HashMap<K, V>
where
    K: PacketCodec + Eq + Hash,
    V: PacketCodec,
{
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        ctx.serialize::<u32>(&(self.len() as u32))?;
        for (key, value) in self.iter() {
            ctx.serialize::<K>(key)?;
            ctx.serialize::<V>(value)?;
        }
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<HashMap<K, V>> {
        let length = ctx.deserialize::<u32>()? as usize;

        // we'd like to do with_capacity here instead of new, but that would
        // mean trusting unsanitized input for the size of an allocation, which
        // is a big nono.
        let mut map = HashMap::new();

        // using the deserialized length here is okay, because if the client
        // lies about the size of the vec, then it'll either be legitimate, or
        // we'll run into an out of bytes error and cut it short.
        for _ in 0..length {
            let key = ctx.deserialize::<K>()?;
            let value = ctx.deserialize::<V>()?;
            map.insert(key, value);
        }

        Ok(map)
    }
}

impl<T: PacketCodec> PacketCodec for Option<T> {
    fn serialize<'buf>(
        &self,
        ctx: &mut PacketSerializerContext<'buf>,
    ) -> PacketSerializeResult<()> {
        match self {
            None => ctx.serialize::<u8>(&0)?,
            Some(inner) => {
                ctx.serialize::<u8>(&1)?;
                ctx.serialize::<T>(inner)?;
            }
        }
        Ok(())
    }

    fn deserialize<'buf>(
        ctx: &mut PacketDeserializerContext<'buf>,
    ) -> PacketDeserializeResult<Self> {
        Ok(match ctx.deserialize::<u8>()? {
            0 => None,
            1 => Some(ctx.deserialize::<T>()?),
            _ => return Err(PacketDeserializeError::InvalidEnumVariant),
        })
    }
}
