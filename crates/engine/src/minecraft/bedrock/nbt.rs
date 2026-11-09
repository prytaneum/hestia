//! A little-endian NBT reader that extracts only what a Bedrock `level.dat`
//! contributes to a [`WorldInfo`](proto::instance::WorldInfo) and skips every
//! other tag. `fastnbt` is big-endian Java only.

use thiserror::Error;

const MAX_DEPTH: usize = 32;

const END: u8 = 0;
const BYTE: u8 = 1;
const SHORT: u8 = 2;
const INT: u8 = 3;
const LONG: u8 = 4;
const FLOAT: u8 = 5;
const DOUBLE: u8 = 6;
const BYTE_ARRAY: u8 = 7;
const STRING: u8 = 8;
const LIST: u8 = 9;
const COMPOUND: u8 = 10;
const INT_ARRAY: u8 = 11;
const LONG_ARRAY: u8 = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NbtError {
    #[error("nbt data ends early")]
    Truncated,
    #[error("nbt nests deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("nbt holds an invalid tag, length or string")]
    Malformed,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Level {
    pub name: Option<String>,
    pub game_type: Option<i32>,
    pub difficulty: Option<i32>,
    pub last_played: Option<i64>,
    pub last_opened_with: Option<Vec<i32>>,
}

pub fn read_level(payload: &[u8]) -> Result<Level, NbtError> {
    let mut reader = Reader { bytes: payload };
    if reader.u8()? != COMPOUND {
        return Err(NbtError::Malformed);
    }
    reader.string()?;
    let mut level = Level::default();
    reader.compound(1, Some(&mut level))?;
    Ok(level)
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], NbtError> {
        if len > self.bytes.len() {
            return Err(NbtError::Truncated);
        }
        let (head, tail) = self.bytes.split_at(len);
        self.bytes = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, NbtError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, NbtError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, NbtError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn length(&mut self) -> Result<usize, NbtError> {
        usize::try_from(self.i32()?).map_err(|_| NbtError::Malformed)
    }

    fn string(&mut self) -> Result<String, NbtError> {
        let len = usize::from(self.u16()?);
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| NbtError::Malformed)
    }

    fn skip_elements(&mut self, count: usize, size: usize) -> Result<(), NbtError> {
        self.take(count.checked_mul(size).ok_or(NbtError::Malformed)?)
            .map(drop)
    }

    fn skip(&mut self, tag: u8, depth: usize) -> Result<(), NbtError> {
        match tag {
            BYTE => self.skip_elements(1, 1),
            SHORT => self.skip_elements(1, 2),
            INT | FLOAT => self.skip_elements(1, 4),
            LONG | DOUBLE => self.skip_elements(1, 8),
            BYTE_ARRAY => {
                let len = self.length()?;
                self.skip_elements(len, 1)
            }
            STRING => self.string().map(drop),
            LIST => self.list(depth).map(drop),
            COMPOUND => self.compound(depth + 1, None),
            INT_ARRAY => {
                let len = self.length()?;
                self.skip_elements(len, 4)
            }
            LONG_ARRAY => {
                let len = self.length()?;
                self.skip_elements(len, 8)
            }
            _ => Err(NbtError::Malformed),
        }
    }

    fn list(&mut self, depth: usize) -> Result<Option<Vec<i32>>, NbtError> {
        let element = self.u8()?;
        let len = self.length()?;
        match element {
            END if len == 0 => Ok(None),
            END => Err(NbtError::Malformed),
            INT => (0..len)
                .map(|_| self.i32())
                .collect::<Result<_, _>>()
                .map(Some),
            _ => {
                if depth >= MAX_DEPTH {
                    return Err(NbtError::TooDeep);
                }
                (0..len)
                    .try_for_each(|_| self.skip(element, depth + 1))
                    .map(|_| None)
            }
        }
    }

    fn compound(&mut self, depth: usize, mut wanted: Option<&mut Level>) -> Result<(), NbtError> {
        if depth > MAX_DEPTH {
            return Err(NbtError::TooDeep);
        }
        loop {
            let tag = self.u8()?;
            if tag == END {
                return Ok(());
            }
            let name = self.string()?;
            let Some(level) = wanted.as_deref_mut() else {
                self.skip(tag, depth)?;
                continue;
            };
            match (name.as_str(), tag) {
                ("LevelName", STRING) => level.name = Some(self.string()?),
                ("GameType", INT) => level.game_type = Some(self.i32()?),
                ("Difficulty", INT) => level.difficulty = Some(self.i32()?),
                ("LastPlayed", LONG) => level.last_played = Some(self.i64()?),
                ("lastOpenedWithVersion", LIST) => level.last_opened_with = self.list(depth)?,
                _ => self.skip(tag, depth)?,
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn name(out: &mut Vec<u8>, name: &str) {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
    }

    fn tag(out: &mut Vec<u8>, id: u8, key: &str) {
        out.push(id);
        name(out, key);
    }

    /// A root compound holding every tag type around the fields we read.
    pub(crate) fn level_payload(world: &str) -> Vec<u8> {
        let mut nbt = Vec::new();
        tag(&mut nbt, COMPOUND, "");
        tag(&mut nbt, BYTE, "b");
        nbt.push(1);
        tag(&mut nbt, SHORT, "s");
        nbt.extend_from_slice(&7i16.to_le_bytes());
        tag(&mut nbt, INT, "GameType");
        nbt.extend_from_slice(&1i32.to_le_bytes());
        tag(&mut nbt, FLOAT, "f");
        nbt.extend_from_slice(&1.5f32.to_le_bytes());
        tag(&mut nbt, DOUBLE, "d");
        nbt.extend_from_slice(&2.5f64.to_le_bytes());
        tag(&mut nbt, BYTE_ARRAY, "ba");
        nbt.extend_from_slice(&3i32.to_le_bytes());
        nbt.extend_from_slice(&[1, 2, 3]);
        tag(&mut nbt, STRING, "LevelName");
        name(&mut nbt, world);
        tag(&mut nbt, LIST, "strings");
        nbt.push(STRING);
        nbt.extend_from_slice(&2i32.to_le_bytes());
        name(&mut nbt, "a");
        name(&mut nbt, "bc");
        tag(&mut nbt, COMPOUND, "nested");
        tag(&mut nbt, LONG, "LastPlayed");
        nbt.extend_from_slice(&1i64.to_le_bytes());
        nbt.push(END);
        tag(&mut nbt, INT_ARRAY, "ia");
        nbt.extend_from_slice(&1i32.to_le_bytes());
        nbt.extend_from_slice(&9i32.to_le_bytes());
        tag(&mut nbt, LONG_ARRAY, "la");
        nbt.extend_from_slice(&1i32.to_le_bytes());
        nbt.extend_from_slice(&9i64.to_le_bytes());
        tag(&mut nbt, INT, "Difficulty");
        nbt.extend_from_slice(&3i32.to_le_bytes());
        tag(&mut nbt, LONG, "LastPlayed");
        nbt.extend_from_slice(&1_751_000_000i64.to_le_bytes());
        tag(&mut nbt, LIST, "lastOpenedWithVersion");
        nbt.push(INT);
        nbt.extend_from_slice(&5i32.to_le_bytes());
        for part in [1i32, 21, 44, 1, 0] {
            nbt.extend_from_slice(&part.to_le_bytes());
        }
        nbt.push(END);
        nbt
    }

    #[test]
    fn every_tag_type_is_skipped_and_the_fields_are_read() {
        let level = read_level(&level_payload("Mine")).unwrap();
        assert_eq!(
            level,
            Level {
                name: Some("Mine".into()),
                game_type: Some(1),
                difficulty: Some(3),
                last_played: Some(1_751_000_000),
                last_opened_with: Some(vec![1, 21, 44, 1, 0]),
            }
        );
    }

    #[test]
    fn truncated_input_is_an_error() {
        let payload = level_payload("Mine");
        for cut in [0, 1, 5, payload.len() / 2, payload.len() - 1] {
            assert!(read_level(&payload[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn negative_lengths_are_an_error() {
        let mut nbt = Vec::new();
        tag(&mut nbt, COMPOUND, "");
        tag(&mut nbt, BYTE_ARRAY, "ba");
        nbt.extend_from_slice(&(-1i32).to_le_bytes());
        nbt.push(END);
        assert_eq!(read_level(&nbt), Err(NbtError::Malformed));
    }

    #[test]
    fn excessive_nesting_is_an_error() {
        let mut nbt = Vec::new();
        tag(&mut nbt, COMPOUND, "");
        for _ in 0..MAX_DEPTH + 1 {
            tag(&mut nbt, COMPOUND, "c");
        }
        assert_eq!(read_level(&nbt), Err(NbtError::TooDeep));
    }

    #[test]
    fn a_root_that_is_not_a_compound_is_an_error() {
        assert_eq!(read_level(&[STRING, 0, 0]), Err(NbtError::Malformed));
    }
}
