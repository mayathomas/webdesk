
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: String, // base64编码的区域数据
}
