//! 本机 API 返回值序列化与客户端可见路径/任务元数据脱敏投影。

use super::*;

pub(super) fn value<T: serde::Serialize>(result: Result<T, String>) -> Result<Value, ApiError> {
    serde_json::to_value(result.map_err(|error| ApiError::operation(&error))?)
        .map_err(|_| ApiError::internal())
}
pub(super) fn is_client_path_authorized(path_str: &str, client: &ApiClient) -> bool {
    if !client.scopes.contains(&Scope::FilesRead)
        || path_str.is_empty()
        || path_str == "[REDACTED]"
        || !Path::new(path_str).is_absolute()
    {
        return false;
    }
    let Ok(target) = canonicalize_existing(path_str).or_else(|_| canonicalize_for_save(path_str))
    else {
        return false;
    };
    if openunpack_local_api::is_trusted_all_paths() {
        return true;
    }
    client
        .read_roots
        .iter()
        .chain(&client.write_roots)
        .any(|root| {
            canonicalize_existing(root)
                .or_else(|_| canonicalize_for_save(root))
                .is_ok_and(|root| {
                    path_eq_case_insensitive(&target, &root)
                        || root.is_dir() && path_starts_with_case_insensitive(&target, &root)
                })
        })
}

pub(super) fn redact_job_metadata(output: &mut Value, client: &ApiClient) {
    match output {
        Value::Object(fields) => {
            if !client.scopes.contains(&Scope::SettingsRead) && fields.contains_key("options") {
                fields.insert("options".into(), serde_json::json!(Settings::default()));
            }
            if !client.scopes.contains(&Scope::VaultRead) {
                if fields.contains_key("passwordLabel") {
                    fields.insert("passwordLabel".into(), Value::Null);
                }
                if fields.contains_key("passwordGroup") {
                    fields.insert("passwordGroup".into(), Value::String(String::new()));
                }
            }
            // Local hook commands and the application database location are
            // never part of a client's file grants.
            if fields.contains_key("onComplete") {
                fields.insert("onComplete".into(), Value::Null);
            }
            if fields.contains_key("dataDirectory") {
                fields.insert("dataDirectory".into(), Value::String("[REDACTED]".into()));
            }
            if let Some(path) = fields
                .get("source")
                .and_then(|source| source.get("path"))
                .and_then(Value::as_str)
                && !is_client_path_authorized(path, client)
                && let Some(nodes) = fields.get_mut("nodes").and_then(Value::as_array_mut)
            {
                nodes.clear();
            }
            // Every location has its own boundary. An approved source says
            // nothing about its sibling volumes, output or cache directories.
            for key in [
                "path",
                "sourcePath",
                "archivePath",
                "currentFile",
                "outputPath",
                "output_path",
                "workspace",
                "outputRoot",
                "cacheRoot",
                "dataDirectory",
            ] {
                if let Some(path) = fields.get(key).and_then(Value::as_str)
                    && !path.is_empty()
                    && !is_client_path_authorized(path, client)
                {
                    fields.insert(
                        key.into(),
                        if matches!(
                            key,
                            "outputPath" | "output_path" | "workspace" | "currentFile"
                        ) {
                            Value::Null
                        } else {
                            Value::String("[REDACTED]".into())
                        },
                    );
                }
            }
            for key in ["volumes", "watchedFolders"] {
                if let Some(paths) = fields.get_mut(key).and_then(Value::as_array_mut) {
                    for path in paths {
                        if path
                            .as_str()
                            .is_some_and(|path| !is_client_path_authorized(path, client))
                        {
                            *path = Value::String("[REDACTED]".into());
                        }
                    }
                }
            }
            for value in fields.values_mut() {
                redact_job_metadata(value, client);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_job_metadata(value, client);
            }
        }
        _ => {}
    }
}
pub(super) fn snapshot_value(
    result: Result<AppSnapshot, String>,
    client: &ApiClient,
) -> Result<Value, ApiError> {
    let mut output = value(result)?;
    let fields = output.as_object_mut().ok_or_else(ApiError::internal)?;
    if !client.scopes.contains(&Scope::VaultRead) {
        fields.insert("passwords".into(), serde_json::json!([]));
        fields.insert("vaultError".into(), Value::Null);
    }
    if !client.scopes.contains(&Scope::SettingsRead) {
        fields.insert("settings".into(), serde_json::json!(Settings::default()));
    }
    redact_job_metadata(&mut output, client);
    Ok(output)
}
pub(super) fn project_progress_paths(
    mut update: openunpack_protocol::JobProgressUpdate,
    client: &ApiClient,
) -> openunpack_protocol::JobProgressUpdate {
    if update
        .current_file
        .as_ref()
        .is_some_and(|path| !is_client_path_authorized(path, client))
    {
        update.current_file = None;
    }
    update
}
pub(super) fn project_need_input_paths(
    mut event: openunpack_protocol::NeedPasswordEvent,
    client: &ApiClient,
) -> openunpack_protocol::NeedPasswordEvent {
    if !is_client_path_authorized(&event.archive_path, client) {
        event.archive_path.clear();
    }
    event
}
pub(super) fn job_value<T: serde::Serialize>(
    result: Result<T, String>,
    client: &ApiClient,
) -> Result<Value, ApiError> {
    let mut output = value(result)?;
    redact_job_metadata(&mut output, client);
    Ok(output)
}
pub(super) fn owned_request(client: &ApiClient, id: String) -> Result<String, ApiError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || matches!(v, b'-' | b'_'))
    {
        return Err(ApiError::new(
            400,
            "invalid_request",
            "请求编号必须是 1 至 64 位字母、数字、横线或下划线。",
        ));
    }
    Ok(format!("api-{}-{id}", client.id))
}
