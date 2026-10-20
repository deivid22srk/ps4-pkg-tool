use std::path::PathBuf;

use jni::objects::{JObject, JString, JValue};
use jni::sys::{jint, jstring};
use jni::JNIEnv;
use serde_json::json;

use crate::error::CoreError;
use crate::pack::{self, PackOptions};
use crate::pkg;
use crate::progress::{request_cancel, reset_cancel, Progress};

struct JniProgress<'a, 'local> {
    env: &'a mut JNIEnv<'local>,
    callback: &'a JObject<'local>,
}

impl<'a, 'local> Progress for JniProgress<'a, 'local> {
    fn report(&mut self, percent: u8, message: &str) -> crate::Result<()> {
        if crate::is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let text = self
            .env
            .new_string(message)
            .map_err(|e| CoreError::Internal(format!("JNI new_string: {e}")))?;
        let text_obj: JObject = text.into();
        self.env
            .call_method(
                self.callback,
                "onProgress",
                "(ILjava/lang/String;)V",
                &[JValue::Int(percent as i32), JValue::Object(&text_obj)],
            )
            .map_err(|e| {
                let _ = self.env.exception_clear();
                CoreError::Internal(format!("JNI callback: {e}"))
            })?;
        if crate::is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        Ok(())
    }
}

fn path_from_env(env: &mut JNIEnv, value: &JString) -> crate::Result<PathBuf> {
    let text = env
        .get_string(value)
        .map_err(|e| CoreError::Internal(format!("JNI get_string: {e}")))?;
    let text = String::from(text);
    if text.is_empty() {
        return Err(CoreError::InvalidInput("caminho vazio".into()));
    }
    Ok(PathBuf::from(text))
}

fn optional_string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    if value.as_raw().is_null() {
        return None;
    }
    env.get_string(value)
        .ok()
        .map(|s| String::from(s))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn to_java(env: &mut JNIEnv, value: String) -> jstring {
    match env.new_string(value) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn error_json(error: &CoreError) -> String {
    json!({
        "ok": false,
        "code": error.code(),
        "message": error.message(),
    })
    .to_string()
}

#[no_mangle]
pub extern "system" fn Java_com_deivid_pkgtool_core_NativeCore_analyze(
    mut env: JNIEnv,
    _this: JObject,
    path: JString,
) -> jstring {
    let result = (|| {
        let path = path_from_env(&mut env, &path)?;
        let info = pkg::analyze(&path)?;
        serde_json::to_string(&info)
            .map_err(|e| CoreError::Internal(format!("serializar análise: {e}")))
    })();
    let payload = match result {
        Ok(json) => json,
        Err(error) => error_json(&error),
    };
    to_java(&mut env, payload)
}

#[no_mangle]
pub extern "system" fn Java_com_deivid_pkgtool_core_NativeCore_analyzeFd(
    mut env: JNIEnv,
    _this: JObject,
    fd: jint,
) -> jstring {
    let payload = match pkg::analyze_fd(fd as i32) {
        Ok(info) => serde_json::to_string(&info)
            .unwrap_or_else(|e| error_json(&CoreError::Internal(e.to_string()))),
        Err(error) => error_json(&error),
    };
    to_java(&mut env, payload)
}

#[no_mangle]
pub extern "system" fn Java_com_deivid_pkgtool_core_NativeCore_extract(
    mut env: JNIEnv,
    _this: JObject,
    pkg_path: JString,
    output: JString,
    callback: JObject,
) -> jstring {
    reset_cancel();
    let result = (|| {
        let pkg_path = path_from_env(&mut env, &pkg_path)?;
        let output = path_from_env(&mut env, &output)?;
        let mut progress = JniProgress {
            env: &mut env,
            callback: &callback,
        };
        let summary = pkg::extract_summary(&pkg_path, &output, &mut progress)?;
        drop(progress);
        Ok(json!({
            "ok": true,
            "bytes": summary.bytes,
            "fileCount": summary.file_count,
            "contentId": summary.content_id,
            "title": summary.title,
            "message": "Extração concluída",
        }))
    })();
    let payload = match result {
        Ok(value) => value.to_string(),
        Err(error) => error_json(&error),
    };
    to_java(&mut env, payload)
}

#[no_mangle]
pub extern "system" fn Java_com_deivid_pkgtool_core_NativeCore_pack(
    mut env: JNIEnv,
    _this: JObject,
    input: JString,
    output: JString,
    content_id: JString,
    title: JString,
    callback: JObject,
) -> jstring {
    reset_cancel();
    let result = (|| {
        let input = path_from_env(&mut env, &input)?;
        let output = path_from_env(&mut env, &output)?;
        let content_id = optional_string(&mut env, &content_id);
        let title = optional_string(&mut env, &title);
        let options = PackOptions { content_id, title };
        let mut progress = JniProgress {
            env: &mut env,
            callback: &callback,
        };
        let summary = pack::pack(&input, &output, &options, &mut progress)?;
        drop(progress);
        Ok(json!({
            "ok": true,
            "contentId": summary.content_id,
            "title": summary.title,
            "titleId": summary.title_id,
            "fileCount": summary.file_count,
            "inputBytes": summary.input_bytes,
            "pkgSize": summary.pkg_size,
            "message": "PKG criado",
        }))
    })();
    let payload = match result {
        Ok(value) => value.to_string(),
        Err(error) => error_json(&error),
    };
    to_java(&mut env, payload)
}

#[no_mangle]
pub extern "system" fn Java_com_deivid_pkgtool_core_NativeCore_cancel(
    _env: JNIEnv,
    _this: JObject,
) {
    request_cancel();
}

#[no_mangle]
pub extern "system" fn Java_com_deivid_pkgtool_core_NativeCore_version(
    mut env: JNIEnv,
    _this: JObject,
) -> jstring {
    to_java(&mut env, env!("CARGO_PKG_VERSION").to_string())
}

