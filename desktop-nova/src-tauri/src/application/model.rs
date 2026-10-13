use serde::{Deserialize, Serialize};

/// Physical screen placement is presentation state, never Viewer runtime
/// ownership. DPI remains explicit so the application does not reinterpret a
/// CSS rectangle as device pixels.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct PresentationPlacement {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) dpi_scale: f64,
    pub(crate) focused: bool,
}


#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ProductPreviewImage {
    pub(crate) preview_path: String,
    pub(crate) data_url: String,
}

