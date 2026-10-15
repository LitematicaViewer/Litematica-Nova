//! Presentation transport only. Fixed-size metadata accompanies Mach rights;
//! no pixels, scene rules, geometry or renderer implementation travel here.

pub const FRAME_MESSAGE_ID: i32 = 0x4e56_4d31;
pub const ACK_MESSAGE_ID: i32 = 0x4e56_4d32;
pub const INPUT_MESSAGE_ID: i32 = 0x4e56_4d33;
pub const DISPLAY_MESSAGE_ID: i32 = 0x4e56_4d34;
pub const FRAME_BYTES: usize = 64;
pub const ACK_BYTES: usize = 32;
pub const INPUT_BYTES: usize = 128;
pub const DISPLAY_BYTES: usize = 56;
pub const GPU_COMPLETE: u32 = 1;
pub const REAL_GEOMETRY: u32 = 2;
pub const BGRA8_UNORM: u32 = 1;
pub const BGRA8_SRGB: u32 = 2;
pub const MAX_SURFACE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub nonce: [u8; 16],
    pub generation: u64,
    pub epoch: u64,
    pub frame_id: u64,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub flags: u32,
}

impl FrameHeader {
    pub fn encode(self) -> [u8; FRAME_BYTES] {
        let mut bytes = [0; FRAME_BYTES];
        bytes[..4].copy_from_slice(b"NVM1");
        bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
        bytes[8..24].copy_from_slice(&self.nonce);
        bytes[24..32].copy_from_slice(&self.generation.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.epoch.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.frame_id.to_le_bytes());
        bytes[48..52].copy_from_slice(&self.width.to_le_bytes());
        bytes[52..56].copy_from_slice(&self.height.to_le_bytes());
        bytes[56..60].copy_from_slice(&self.format.to_le_bytes());
        bytes[60..64].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != FRAME_BYTES || &bytes[..4] != b"NVM1" || u32_at(bytes, 4) != 1 {
            return Err("invalid native frame protocol");
        }
        let frame = Self {
            nonce: bytes[8..24].try_into().unwrap(),
            generation: u64_at(bytes, 24),
            epoch: u64_at(bytes, 32),
            frame_id: u64_at(bytes, 40),
            width: u32_at(bytes, 48),
            height: u32_at(bytes, 52),
            format: u32_at(bytes, 56),
            flags: u32_at(bytes, 60),
        };
        if frame.generation == 0
            || frame.frame_id == 0
            || frame.flags & GPU_COMPLETE == 0
            || frame.flags & !(GPU_COMPLETE | REAL_GEOMETRY) != 0
            || !matches!(frame.format, BGRA8_UNORM | BGRA8_SRGB)
            || frame.width == 0
            || frame.height == 0
            || frame.width > 16_384
            || frame.height > 16_384
            || u64::from(frame.width) * u64::from(frame.height) * 4 > MAX_SURFACE_BYTES
        {
            return Err("invalid or incomplete native frame");
        }
        Ok(frame)
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[derive(Debug)]
pub struct FrameGate {
    pub nonce: [u8; 16],
    pub generation: u64,
    pub epoch: u64,
    pub visible: bool,
    pub width: u32,
    pub height: u32,
    pub last_frame: u64,
}

impl FrameGate {
    pub fn set_presentation(&mut self, epoch: u64, visible: bool, width: u32, height: u32) -> bool {
        if epoch < self.epoch {
            return false;
        }
        if epoch != self.epoch {
            self.last_frame = 0;
        }
        self.epoch = epoch;
        self.visible = visible;
        self.width = width;
        self.height = height;
        true
    }
    pub fn accepts(&self, frame: FrameHeader) -> bool {
        self.visible
            && frame.nonce == self.nonce
            && frame.generation == self.generation
            && frame.epoch == self.epoch
            && frame.width == self.width
            && frame.height == self.height
            && frame.frame_id > self.last_frame
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum AckStatus {
    GpuCopyComplete = 0,
    Discarded = 1,
    Failed = 2,
}

pub fn encode_ack(frame: FrameHeader, status: AckStatus) -> [u8; ACK_BYTES] {
    let mut bytes = [0; ACK_BYTES];
    bytes[..4].copy_from_slice(b"NVMA");
    bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
    bytes[8..16].copy_from_slice(&frame.generation.to_le_bytes());
    bytes[16..24].copy_from_slice(&frame.frame_id.to_le_bytes());
    bytes[24..28].copy_from_slice(&(status as u32).to_le_bytes());
    bytes
}

/// An independently observed CAMetalDrawable presentation, not a GPU upload or
/// command-buffer completion. presentedTime is the system media clock.
pub fn encode_display(frame: FrameHeader, presented_time: f64) -> [u8; DISPLAY_BYTES] {
    let mut bytes = [0; DISPLAY_BYTES];
    bytes[..4].copy_from_slice(b"NVMP");
    bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
    bytes[8..24].copy_from_slice(&frame.nonce);
    bytes[24..32].copy_from_slice(&frame.generation.to_le_bytes());
    bytes[32..40].copy_from_slice(&frame.epoch.to_le_bytes());
    bytes[40..48].copy_from_slice(&frame.frame_id.to_le_bytes());
    bytes[48..56].copy_from_slice(&presented_time.to_le_bytes());
    bytes
}

/// kind: move=1, down=2, up=3, wheel=4, keyDown=5, keyUp=6, focus=7.
/// Coordinates and deltas use physical pixels. code is button/keycode; modifier
/// bits retain NSEventModifierFlags. flags bit0 means repeat or focus=true.
pub struct InputPacket<'a> {
    pub nonce: [u8; 16],
    pub generation: u64,
    pub epoch: u64,
    pub sequence: u64,
    pub kind: u32,
    pub flags: u32,
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub code: u32,
    pub modifiers: u32,
    pub text: &'a str,
}

impl InputPacket<'_> {
    pub fn encode(&self) -> [u8; INPUT_BYTES] {
        let mut bytes = [0; INPUT_BYTES];
        bytes[..4].copy_from_slice(b"NVMI");
        bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
        bytes[8..24].copy_from_slice(&self.nonce);
        for (offset, value) in [(24, self.generation), (32, self.epoch), (40, self.sequence)] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48..52].copy_from_slice(&self.kind.to_le_bytes());
        bytes[52..56].copy_from_slice(&self.flags.to_le_bytes());
        for (offset, value) in [(56, self.x), (64, self.y), (72, self.dx), (80, self.dy)] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[88..92].copy_from_slice(&self.code.to_le_bytes());
        bytes[92..96].copy_from_slice(&self.modifiers.to_le_bytes());
        let mut length = self.text.len().min(28);
        while !self.text.is_char_boundary(length) {
            length -= 1;
        }
        bytes[96..100].copy_from_slice(&(length as u32).to_le_bytes());
        bytes[100..100 + length].copy_from_slice(&self.text.as_bytes()[..length]);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> FrameHeader {
        FrameHeader {
            nonce: [7; 16],
            generation: 2,
            epoch: 3,
            frame_id: 1,
            width: 1280,
            height: 720,
            format: BGRA8_SRGB,
            flags: GPU_COMPLETE,
        }
    }
    fn gate() -> FrameGate {
        FrameGate {
            nonce: [7; 16],
            generation: 2,
            epoch: 3,
            visible: true,
            width: 1280,
            height: 720,
            last_frame: 0,
        }
    }
    #[test]
    fn metadata_roundtrip_without_pixels() {
        let original = frame();
        assert_eq!(FrameHeader::decode(&original.encode()).unwrap(), original);
        assert_eq!(original.encode().len(), 64);
    }
    #[test]
    fn hide_and_new_page_lease_discard_late_frames() {
        let mut state = gate();
        assert!(state.accepts(frame()));
        state.visible = false;
        assert!(!state.accepts(frame()));
        state.epoch = 4;
        state.visible = true;
        assert!(!state.accepts(frame()));
        let mut next = frame();
        next.epoch = 4;
        assert!(state.accepts(next));
    }
    #[test]
    fn changed_scene_dpi_and_reordered_frames_are_not_presented() {
        let mut state = gate();
        state.generation += 1;
        assert!(!state.accepts(frame()));
        state = gate();
        state.width = 1920;
        assert!(!state.accepts(frame()));
        state = gate();
        state.last_frame = 2;
        assert!(!state.accepts(frame()));
        let mut wrong = frame();
        wrong.nonce[0] ^= 1;
        assert!(!gate().accepts(wrong));
    }
    #[test]
    fn new_owner_frame_counter_restarts_but_old_owner_cannot_advance_state() {
        let mut state = gate();
        state.last_frame = 100;
        assert!(state.set_presentation(4, true, 1280, 720));
        let mut fresh = frame();
        fresh.epoch = 4;
        assert!(state.accepts(fresh));
        assert!(!state.set_presentation(3, true, 320, 200));
        assert_eq!((state.epoch, state.width, state.height), (4, 1280, 720));
        assert!(!state.accepts(frame()));
    }
    #[test]
    fn unfinished_invalid_or_over_budget_surface_is_rejected() {
        let mut value = frame();
        value.flags = 0;
        assert!(FrameHeader::decode(&value.encode()).is_err());
        value = frame();
        value.width = 16_384;
        value.height = 16_384;
        assert!(FrameHeader::decode(&value.encode()).is_err());
        value = frame();
        value.format = 99;
        assert!(FrameHeader::decode(&value.encode()).is_err());
    }
    #[test]
    fn input_truncation_preserves_utf8_and_ack_releases_exact_slot() {
        let packet = InputPacket {
            nonce: [7; 16],
            generation: 2,
            epoch: 3,
            sequence: 4,
            kind: 5,
            flags: 0,
            x: 1.0,
            y: 2.0,
            dx: 0.0,
            dy: 0.0,
            code: 9,
            modifiers: 0,
            text: "中文输入不会截断半个字符且长度有界",
        };
        let bytes = packet.encode();
        let length = u32_at(&bytes, 96) as usize;
        assert!(length <= 28);
        assert!(std::str::from_utf8(&bytes[100..100 + length]).is_ok());
        let ack = encode_ack(frame(), AckStatus::Discarded);
        assert_eq!(u64_at(&ack, 8), 2);
        assert_eq!(u64_at(&ack, 16), 1);
        assert_eq!(u32_at(&ack, 24), 1);
    }
}
