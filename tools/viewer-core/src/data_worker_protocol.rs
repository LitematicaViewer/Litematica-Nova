//! Local data-worker wire contract. Shared by the Core and Application Host.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

pub const MAX_FRAME: usize = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DataKind {
    Analysis,
    Stats,
    Materials,
    MaterialDetail,
    MaterialRecipes,
    StatisticsRules,
    Stockpile,
    VersionConvert,
    Cancel,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataRequest {
    pub operation_id: String,
    pub kind: DataKind,
    pub input: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataResponse {
    pub operation_id: String,
    pub state: DataState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum DataState {
    Pending,
    Running,
    Ready,
    Failed,
    Cancelled,
}
impl DataResponse {
    pub fn state(id: &str, state: DataState) -> Self {
        Self {
            operation_id: id.into(),
            state,
            progress: None,
            result: None,
            error: None,
        }
    }
}
pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Data response exceeds bounded IPC frame",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}
pub fn read_frame<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Data request exceeds bounded IPC frame",
        ));
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_wire_preserves_operation_and_rejects_oversized_input() {
        let request = DataRequest {
            operation_id: "task-A".into(),
            kind: DataKind::Stats,
            input: serde_json::json!({"sourcePath":"example.litematic"}),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request).unwrap();
        let actual: DataRequest = read_frame(&mut bytes.as_slice()).unwrap();
        assert_eq!(actual.operation_id, "task-A");
        assert_eq!(actual.kind, DataKind::Stats);
        let header = ((MAX_FRAME + 1) as u32).to_le_bytes();
        assert!(read_frame::<DataRequest>(&mut header.as_slice()).is_err());
    }
}
