use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformConfig {
    pub name: String,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub input_api: String,
    pub render_backends: Vec<String>,
    pub supported_extensions: Vec<String>,
}

#[cfg(target_os = "windows")]
pub fn init_platform() -> PlatformConfig {
    PlatformConfig {
        name: "windows".to_string(),
        data_dir: dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("OxideSFC"),
        config_dir: dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("OxideSFC"),
        input_api: "gilrs".to_string(),
        render_backends: vec!["webgpu".to_string(), "webgl".to_string(), "d3d11".to_string()],
        supported_extensions: vec![
            "sfc".to_string(),
            "smc".to_string(),
            "fig".to_string(),
            "swc".to_string(),
        ],
    }
}

#[cfg(target_os = "macos")]
pub fn init_platform() -> PlatformConfig {
    PlatformConfig {
        name: "macos".to_string(),
        data_dir: dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("OxideSFC"),
        config_dir: dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("OxideSFC"),
        input_api: "gilrs".to_string(),
        render_backends: vec!["webgpu".to_string(), "webgl".to_string(), "metal".to_string()],
        supported_extensions: vec![
            "sfc".to_string(),
            "smc".to_string(),
            "fig".to_string(),
            "swc".to_string(),
        ],
    }
}

#[cfg(target_os = "linux")]
pub fn init_platform() -> PlatformConfig {
    PlatformConfig {
        name: "linux".to_string(),
        data_dir: dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("oxidesfc"),
        config_dir: dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("oxidesfc"),
        input_api: "gilrs".to_string(),
        render_backends: vec!["webgpu".to_string(), "webgl".to_string(), "opengl".to_string()],
        supported_extensions: vec![
            "sfc".to_string(),
            "smc".to_string(),
            "fig".to_string(),
            "swc".to_string(),
        ],
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn init_platform() -> PlatformConfig {
    PlatformConfig {
        name: "unknown".to_string(),
        data_dir: PathBuf::from("."),
        config_dir: PathBuf::from("."),
        input_api: "unknown".to_string(),
        render_backends: vec!["webgl".to_string()],
        supported_extensions: vec![
            "sfc".to_string(),
            "smc".to_string(),
            "fig".to_string(),
            "swc".to_string(),
        ],
    }
}
