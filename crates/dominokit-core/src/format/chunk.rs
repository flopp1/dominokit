//! Chunk grammar (§2.2): `chunk := id:u16  size:u32  data:[u8; size]`.
//!
//! Parsing is tolerant: a chunk whose declared size runs past the end of its
//! parent stops the parse there and the remaining bytes are returned as
//! `trailing` (with a warning flag), rather than losing the whole sequence.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub tag: u16,
    pub body: Vec<u8>,
}

impl Item {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.body.len());
        out.extend_from_slice(&self.tag.to_le_bytes());
        out.extend_from_slice(&(self.body.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.body);
        out
    }

    pub fn u32_value(&self) -> Option<u32> {
        if self.body.len() == 4 {
            Some(u32::from_le_bytes(self.body[0..4].try_into().unwrap()))
        } else {
            None
        }
    }

    pub fn u16_value(&self) -> Option<u16> {
        if self.body.len() == 2 {
            Some(u16::from_le_bytes(self.body[0..2].try_into().unwrap()))
        } else {
            None
        }
    }

    pub fn u8_value(&self) -> Option<u8> {
        if self.body.len() == 1 {
            Some(self.body[0])
        } else {
            None
        }
    }

    pub fn set_u32_value(&mut self, v: u32) {
        self.body = v.to_le_bytes().to_vec();
    }

    pub fn set_u16_value(&mut self, v: u16) {
        self.body = v.to_le_bytes().to_vec();
    }

    pub fn set_u8_value(&mut self, v: u8) {
        self.body = vec![v];
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParseResult {
    pub items: Vec<Item>,
    /// Bytes left over when a chunk's declared size ran past the end.
    pub trailing: Vec<u8>,
}

impl ParseResult {
    pub fn is_clean(&self) -> bool {
        self.trailing.is_empty()
    }
}

/// Parse a chunk sequence, keeping any unparsable tail.
pub fn read_all(data: &[u8]) -> ParseResult {
    let mut items = Vec::new();
    let mut i = 0usize;
    while i + 6 <= data.len() {
        let tag = u16::from_le_bytes(data[i..i + 2].try_into().unwrap());
        let size = u32::from_le_bytes(data[i + 2..i + 6].try_into().unwrap());
        let end = match i
            .checked_add(6)
            .and_then(|n| n.checked_add(size as usize))
        {
            Some(e) if e <= data.len() => e,
            _ => break,
        };
        items.push(Item {
            tag,
            body: data[i + 6..end].to_vec(),
        });
        i = end;
    }
    ParseResult {
        items,
        trailing: data[i..].to_vec(),
    }
}

/// Convenience: parse and keep only the items (old strict-ish behaviour for
/// inner scans where the caller ignores trailing bytes).
pub fn items(data: &[u8]) -> Vec<Item> {
    read_all(data).items
}

pub fn item(tag: u16, body: &[u8]) -> Vec<u8> {
    Item {
        tag,
        body: body.to_vec(),
    }
    .encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_items() {
        let bytes = item(1001, &[1, 2, 3, 4]);
        let r = read_all(&bytes);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].tag, 1001);
        assert_eq!(r.items[0].u32_value(), Some(0x04030201));
        assert!(r.is_clean());
    }

    #[test]
    fn keeps_trailing_bytes() {
        let mut bytes = item(1000, &[]);
        bytes.extend_from_slice(&[0xde, 0xad]);
        let r = read_all(&bytes);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.trailing, vec![0xde, 0xad]);
    }

    #[test]
    fn short_header_is_trailing() {
        let r = read_all(&[1, 2, 3]);
        assert!(r.items.is_empty());
        assert_eq!(r.trailing, vec![1, 2, 3]);
    }

    #[test]
    fn item_encode_is_identity() {
        let bytes = item(2001, &[9, 8, 7]);
        assert_eq!(read_all(&bytes).items[0].encode(), bytes);
    }
}
