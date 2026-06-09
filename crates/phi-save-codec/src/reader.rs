use crate::error::{CodecError, Result};
use alloc::borrow::ToOwned as _;

/// 二进制读取器（从原始字节切片中按顺序读取字段）
pub struct Reader<'a> {
    pub(crate) data: &'a [u8],
    off: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, off: 0 }
    }

    pub fn remain(&self) -> usize {
        self.data.len().saturating_sub(self.off)
    }

    pub fn offset(&self) -> usize {
        self.off
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        if self.remain() < 1 {
            return Err(CodecError::NotEnoughData);
        }
        let b = self.data[self.off];
        self.off += 1;
        Ok(b)
    }

    pub fn read_u16_le(&mut self) -> Result<u16> {
        if self.remain() < 2 {
            return Err(CodecError::NotEnoughData);
        }
        let v = u16::from_le_bytes([self.data[self.off], self.data[self.off + 1]]);
        self.off += 2;
        Ok(v)
    }

    pub fn read_i32_le(&mut self) -> Result<i32> {
        if self.remain() < 4 {
            return Err(CodecError::NotEnoughData);
        }
        let v = i32::from_le_bytes([
            self.data[self.off],
            self.data[self.off + 1],
            self.data[self.off + 2],
            self.data[self.off + 3],
        ]);
        self.off += 4;
        Ok(v)
    }

    pub fn read_f32_le(&mut self) -> Result<f32> {
        if self.remain() < 4 {
            return Err(CodecError::NotEnoughData);
        }
        let v = f32::from_le_bytes([
            self.data[self.off],
            self.data[self.off + 1],
            self.data[self.off + 2],
            self.data[self.off + 3],
        ]);
        self.off += 4;
        Ok(v)
    }

    /// 可变长度整数（与 C 版本一致）
    pub fn read_varshort(&mut self) -> Result<usize> {
        let b0 = self.read_u8()?;
        if b0 < 0x80 {
            Ok(b0 as usize)
        } else {
            let b1 = self.read_u8()?;
            // 与 C 版本保持一致: (b0 & 0x7F) ^ (b1 << 7)
            let v = (((b0 as usize) & 0x7F) ^ ((b1 as usize) << 7)) & 0xFFFF;
            Ok(v)
        }
    }

    /// 读取 `VarInt` 编码的字符串，可选择性地裁掉末尾 N 字节
    pub fn read_string(&mut self, trim_end: usize) -> Result<&'a str> {
        let len = self.read_varshort()?;
        let keep = len.saturating_sub(trim_end);
        if self.remain() < len {
            return Err(CodecError::NotEnoughData);
        }
        let s = &self.data[self.off..self.off + keep];
        self.off += len;
        core::str::from_utf8(s).map_err(|_| CodecError::InvalidUtf8)
    }

    /// 读取字符串并拥有所有权
    pub fn read_owned_string(&mut self, trim_end: usize) -> Result<alloc::string::String> {
        self.read_string(trim_end).map(str::to_owned)
    }

    /// 跳过指定字节数
    pub fn skip(&mut self, n: usize) {
        self.off = self.off.saturating_add(n).min(self.data.len());
    }
}

/// 位操作工具
#[inline]
pub fn get_bit(byte: u8, index: usize) -> bool {
    ((byte >> index) & 1) != 0
}

#[allow(dead_code)]
#[inline]
pub fn set_bit(byte: &mut u8, index: usize, value: bool) {
    if value {
        *byte |= 1 << index;
    } else {
        *byte &= !(1 << index);
    }
}
