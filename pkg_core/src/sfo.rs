use crate::error::{CoreError, Result};

#[derive(Debug, Clone)]
pub enum SfoValue {
    Text(String),
    Int(u32),
}

#[derive(Debug, Clone)]
pub struct SfoEntry {
    pub key: String,
    pub value: SfoValue,
}

#[derive(Debug, Clone, Default)]
pub struct Sfo {
    pub entries: Vec<SfoEntry>,
}

impl Sfo {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 0x14 || &data[0..4] != b"\0PSF" {
            return Err(CoreError::InvalidInput(
                "param.sfo sem assinatura \\0PSF".into(),
            ));
        }
        let key_table = le_u32(data, 0x08)? as usize;
        let data_table = le_u32(data, 0x0C)? as usize;
        let count = le_u32(data, 0x10)? as usize;
        if count > 4096 {
            return Err(CoreError::InvalidInput("param.sfo com entradas demais".into()));
        }
        let entries_end = 0x14usize
            .checked_add(count * 16)
            .ok_or_else(|| CoreError::InvalidInput("param.sfo inválido".into()))?;
        if entries_end > data.len() || key_table > data.len() || data_table > data.len() {
            return Err(CoreError::InvalidInput("param.sfo truncado".into()));
        }

        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let base = 0x14 + i * 16;
            let key_off = le_u16(data, base)? as usize;
            let fmt = le_u16(data, base + 2)?;
            let len = le_u32(data, base + 4)? as usize;
            let data_off = le_u32(data, base + 12)? as usize;

            let key_start = key_table + key_off;
            if key_start >= data.len() {
                continue;
            }
            let key_end = data[key_start..]
                .iter()
                .position(|b| *b == 0)
                .map(|p| key_start + p)
                .unwrap_or(data.len());
            let key = String::from_utf8_lossy(&data[key_start..key_end]).to_string();
            if key.is_empty() {
                continue;
            }

            let value_start = data_table.saturating_add(data_off);
            if value_start > data.len() {
                continue;
            }
            let value_end = value_start.saturating_add(len).min(data.len());
            let value = match fmt {
                0x0404 => {
                    if value_end - value_start >= 4 {
                        SfoValue::Int(le_u32(data, value_start)?)
                    } else {
                        SfoValue::Int(0)
                    }
                }
                0x0204 | 0x0004 => SfoValue::Text(
                    String::from_utf8_lossy(&data[value_start..value_end])
                        .trim_end_matches('\0')
                        .to_string(),
                ),
                _ => continue,
            };
            out.push(SfoEntry { key, value });
        }
        Ok(Self { entries: out })
    }

    pub fn get(&self, key: &str) -> Option<&SfoValue> {
        self.entries
            .iter()
            .find(|e| e.key.eq_ignore_ascii_case(key))
            .map(|e| &e.value)
    }

    pub fn get_text(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(SfoValue::Text(s)) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn get_int(&self, key: &str) -> Option<u32> {
        match self.get(key) {
            Some(SfoValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn set_text(&mut self, key: &str, value: impl Into<String>) {
        self.set(key, SfoValue::Text(value.into()));
    }

    pub fn set_int(&mut self, key: &str, value: u32) {
        self.set(key, SfoValue::Int(value));
    }

    fn set(&mut self, key: &str, value: SfoValue) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.key.eq_ignore_ascii_case(key))
        {
            entry.value = value;
        } else {
            self.entries.push(SfoEntry {
                key: key.to_string(),
                value,
            });
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let count = self.entries.len();
        let header_size = 0x14 + count * 16;

        let mut key_table = Vec::new();
        let mut key_offsets = Vec::with_capacity(count);
        for entry in &self.entries {
            key_offsets.push(key_table.len() as u16);
            key_table.extend_from_slice(entry.key.as_bytes());
            key_table.push(0);
        }

        let mut data_table = Vec::new();
        let mut values = Vec::with_capacity(count);
        for entry in &self.entries {
            match &entry.value {
                SfoValue::Text(text) => {
                    let start = data_table.len();
                    data_table.extend_from_slice(text.as_bytes());
                    data_table.push(0);
                    while data_table.len() % 4 != 0 {
                        data_table.push(0);
                    }
                    let max_len = data_table.len() - start;
                    values.push(((text.len() + 1) as u32, max_len as u32, start as u32));
                }
                SfoValue::Int(value) => {
                    let start = data_table.len();
                    data_table.extend_from_slice(&value.to_le_bytes());
                    values.push((4u32, 4u32, start as u32));
                }
            }
        }

        let key_table_offset = header_size as u32;
        while (header_size + key_table.len()) % 4 != 0 {
            key_table.push(0);
        }
        let data_table_offset = (header_size + key_table.len()) as u32;

        let mut out = Vec::with_capacity(data_table_offset as usize + data_table.len());
        out.extend_from_slice(b"\0PSF");
        out.extend_from_slice(&0x0000_0101u32.to_le_bytes());
        out.extend_from_slice(&key_table_offset.to_le_bytes());
        out.extend_from_slice(&data_table_offset.to_le_bytes());
        out.extend_from_slice(&(count as u32).to_le_bytes());
        for (i, entry) in self.entries.iter().enumerate() {
            let (len, max_len, offset) = values[i];
            out.extend_from_slice(&key_offsets[i].to_le_bytes());
            let fmt: u16 = if matches!(&entry.value, SfoValue::Int(_)) {
                0x0404
            } else {
                0x0204
            };
            out.extend_from_slice(&fmt.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&max_len.to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
        }
        out.extend_from_slice(&key_table);
        out.extend_from_slice(&data_table);
        out
    }

    /// Cria um param.sfo mínimo quando a pasta de entrada não tem um.
    pub fn default_for(title: &str, title_id: &str, content_id: &str) -> Self {
        let mut sfo = Sfo::default();
        sfo.set_int("APP_TYPE", 1);
        sfo.set_text("APP_VER", "01.00");
        sfo.set_int("ATTRIBUTE", 0);
        sfo.set_int("ATTRIBUTE2", 0);
        sfo.set_text("CATEGORY", "gd");
        sfo.set_text("CONTENT_ID", content_id);
        sfo.set_int("PARENTAL_LEVEL", 5);
        sfo.set_int("PUBTOOLVER", 0x0289_0000);
        sfo.set_int("SYSTEM_VER", 0);
        sfo.set_text("TITLE", title);
        sfo.set_text("TITLE_ID", title_id);
        sfo.set_text("VERSION", "01.00");
        sfo
    }
}

fn le_u16(data: &[u8], off: usize) -> Result<u16> {
    data.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| CoreError::InvalidInput("param.sfo truncado".into()))
}

fn le_u32(data: &[u8], off: usize) -> Result<u32> {
    data.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| CoreError::InvalidInput("param.sfo truncado".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfo_roundtrip() {
        let sfo = Sfo::default_for("Meu Jogo", "CUSA12345", "UP0000-CUSA12345_00-MEUJOGO00000000");
        let bytes = sfo.serialize();
        let parsed = Sfo::parse(&bytes).unwrap();
        assert_eq!(parsed.get_text("TITLE").unwrap(), "Meu Jogo");
        assert_eq!(parsed.get_text("TITLE_ID").unwrap(), "CUSA12345");
        assert_eq!(parsed.get_int("APP_TYPE").unwrap(), 1);
    }
}
