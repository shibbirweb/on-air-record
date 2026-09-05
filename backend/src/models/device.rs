//! An audio input the host machine exposes.

/// `cpal` identifies devices by name only, and that name is the only handle that survives a process
/// restart on all three platforms, so it doubles as the persisted id.
#[derive(Debug, Clone)]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// False when the device is remembered in settings but is not plugged in right now.
    pub available: bool,
    pub channels: u16,
    pub sample_rate: u32,
}

impl InputDevice {
    /// Placeholder entry for a configured device that is currently missing, so the UI can explain why
    /// capture will not start instead of silently selecting something else.
    pub fn unavailable(device_id: impl Into<String>) -> Self {
        let id = device_id.into();
        Self {
            name: id.clone(),
            id,
            is_default: false,
            available: false,
            channels: 0,
            sample_rate: 0,
        }
    }
}
