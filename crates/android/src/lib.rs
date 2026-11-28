//! Ezoo Download for Android: the same download engine as the Windows
//! program, called by the app (Kotlin) through JNI.
//!
//! The app's `com.ezoo.download.engine.Engine` object declares these
//! functions (`external fun`). Lists come back as JSON text.

use jni::objects::{JObject, JString};
use jni::sys::{jboolean, jlong, jlongArray, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use swiftget_engine::{Manager, NewDownload, RequestHeaders};

struct Engine {
    _rt: tokio::runtime::Runtime,
    mgr: Arc<Manager>,
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

fn mgr() -> Option<&'static Arc<Manager>> {
    ENGINE.get().map(|e| &e.mgr)
}

fn text(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    env.get_string(s).ok().map(String::from).filter(|s| !s.trim().is_empty())
}

fn jtext(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Starts the engine once: `data_dir` keeps the list and the settings (the
/// app's own folder), `download_dir` gets the files, each kind in its
/// folder (Download/Ezoo/Video, /Music, ...).
fn init(data_dir: PathBuf, download_dir: PathBuf) -> anyhow::Result<()> {
    if ENGINE.get().is_some() {
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().thread_name("ezoo").build()?;
    let mgr = Manager::new(rt.handle().clone(), data_dir)?;
    let mut s = mgr.settings();
    if s.download_dir != download_dir || !s.category_folders {
        s.download_dir = download_dir;
        s.category_folders = true;
        // a phone: fewer connections at a time than a computer
        s.connections = s.connections.min(16);
        mgr.set_settings(s)?;
    }
    let _ = ENGINE.set(Engine { _rt: rt, mgr });
    Ok(())
}

#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_init<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    data_dir: JString<'l>,
    download_dir: JString<'l>,
) -> jstring {
    let (Some(data), Some(dl)) = (text(&mut env, &data_dir), text(&mut env, &download_dir)) else {
        return jtext(&mut env, "no folder was given");
    };
    match init(PathBuf::from(data), PathBuf::from(dl)) {
        Ok(()) => std::ptr::null_mut(),
        Err(e) => jtext(&mut env, &format!("{e:#}")),
    }
}

/// A new download of `url`, started at once. The name comes from the
/// server when `file_name` is empty. Returns its number (0: not started).
#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_add<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    url: JString<'l>,
    file_name: JString<'l>,
    referer: JString<'l>,
    cookies: JString<'l>,
    user_agent: JString<'l>,
) -> jlong {
    let Some(m) = mgr() else { return 0 };
    let Some(url) = text(&mut env, &url) else { return 0 };
    let headers = RequestHeaders {
        referer: text(&mut env, &referer),
        cookies: text(&mut env, &cookies),
        user_agent: text(&mut env, &user_agent),
        ..Default::default()
    };
    let req = NewDownload {
        url: url.trim().to_string(),
        file_name: text(&mut env, &file_name),
        headers,
        start: true,
        ..Default::default()
    };
    m.add(req) as jlong
}

/// The address is in the list already (its number, else 0).
#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_find<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    url: JString<'l>,
) -> jlong {
    let Some(m) = mgr() else { return 0 };
    let Some(url) = text(&mut env, &url) else { return 0 };
    m.find_url(url.trim()).map(|id| id as jlong).unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_start(_env: JNIEnv, _this: JObject, id: jlong) {
    if let Some(m) = mgr() {
        m.start(id as u64);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_pause(_env: JNIEnv, _this: JObject, id: jlong) {
    if let Some(m) = mgr() {
        m.pause(id as u64);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_restart(_env: JNIEnv, _this: JObject, id: jlong) {
    if let Some(m) = mgr() {
        m.restart(id as u64);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_remove(
    _env: JNIEnv,
    _this: JObject,
    id: jlong,
    delete_file: jboolean,
) {
    if let Some(m) = mgr() {
        m.remove(id as u64, delete_file != JNI_FALSE);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_startAll(_env: JNIEnv, _this: JObject) {
    if let Some(m) = mgr() {
        m.start_all();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_pauseAll(_env: JNIEnv, _this: JObject) {
    if let Some(m) = mgr() {
        m.pause_all();
    }
}

/// Something is downloading now (the app keeps its service running).
#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_busy(_env: JNIEnv, _this: JObject) -> jboolean {
    let busy = mgr().is_some_and(|m| {
        m.snapshot().iter().any(|d| d.status.is_running() || d.status == swiftget_engine::Status::Queued)
    });
    if busy {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Every download, newest first, as JSON.
#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_list<'l>(mut env: JNIEnv<'l>, _this: JObject<'l>) -> jstring {
    let Some(m) = mgr() else { return jtext(&mut env, "[]") };
    let mut all = m.snapshot();
    all.sort_by(|a, b| b.added_at.cmp(&a.added_at));
    let items: Vec<_> = all
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "name": d.file_name,
                "path": d.save_path.join(&d.file_name),
                "category": d.category,
                "status": format!("{:?}", d.status),
                "error": d.error,
                "total": d.total_size,
                "done": d.downloaded,
                "speed": d.speed as u64,
                "eta": d.eta_secs,
                "url": d.url,
                "waiting": d.waiting,
                "connections": d.active_connections,
                "added": d.added_at.timestamp_millis(),
            })
        })
        .collect();
    let s = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    jtext(&mut env, &s)
}

/// The downloads finished since the last call (for their notification).
#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_takeCompleted<'l>(
    env: JNIEnv<'l>,
    _this: JObject<'l>,
) -> jlongArray {
    let ids: Vec<jlong> = mgr().map(|m| m.take_completed().into_iter().map(|i| i as jlong).collect()).unwrap_or_default();
    match env.new_long_array(ids.len() as i32) {
        Ok(arr) => {
            let _ = env.set_long_array_region(&arr, 0, &ids);
            arr.into_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// The list is written to the disk (the app goes away).
#[no_mangle]
pub extern "system" fn Java_com_ezoo_download_engine_Engine_shutdown(_env: JNIEnv, _this: JObject) {
    if let Some(m) = mgr() {
        m.shutdown();
    }
}
