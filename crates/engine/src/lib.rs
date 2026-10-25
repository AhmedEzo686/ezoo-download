//! Ezoo Download engine.
//!
//! * `segments`       – splitting a file between connections (dynamic segmentation)
//! * `http_download`  – multi-connection downloader with resume and retries
//! * `hls`            – .m3u8 video streams (AES-128 supported, DRM refused)
//! * `manager`        – queue, scheduler, speed meter, persistence
//!
//! The UI only needs [`Manager`].

pub mod category;
pub mod config;
pub mod ef2;
pub mod fileio;
pub mod filename;
pub mod grabber;
pub mod hls;
pub mod http;
pub mod http_download;
pub mod limiter;
pub mod m3u8;
pub mod manager;
pub mod model;
pub mod mux;
pub mod secret;
pub mod segments;
pub mod selfupdate;
pub mod store;
pub mod system;
pub mod tools;
pub mod ytdlp;

pub use config::{AfterDone, Schedule, Settings};
pub use manager::{default_data_dir, DetailsEdit, Event, Manager};
pub use model::*;
