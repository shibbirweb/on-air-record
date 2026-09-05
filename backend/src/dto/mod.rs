//! Data transfer objects.
//!
//! These are the wire shapes, deliberately separate from the domain models. Keeping them apart means a
//! column rename is a repository change rather than a breaking API change, and it gives one obvious place
//! to enforce the `camelCase` convention the frontend expects.

pub mod bookmark_dto;
pub mod device_dto;
pub mod session_dto;
pub mod settings_dto;
pub mod status_dto;
pub mod timeline_dto;

pub use bookmark_dto::{
    BookmarkDto, BookmarkListResponse, CreateBookmarkRequest, UpdateBookmarkRequest,
};
pub use device_dto::{DeviceDto, DeviceListResponse, SelectDeviceRequest};
pub use session_dto::{SessionDto, SessionListResponse, StorageResponse};
pub use settings_dto::{
    SettingsDto, SettingsPatchRequest, TestDirectoryRequest, TestDirectoryResponse,
};
pub use status_dto::{CaptureDto, HealthResponse, LevelsDto, StatusResponse};
pub use timeline_dto::{
    CoverageDto, ExportPlanResponse, ExportQuery, PeaksQuery, PeaksResponse, RecordingDayDto,
    RecordingDaysResponse, TimelineRangeResponse,
};
