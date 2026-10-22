//! Metadata acceptance through the production Coordinator and native Worker.
//! Synthetic archives only; no application user data or extraction output.
#![cfg(windows)]

use openunpack_core::coordinator::Coordinator;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

fn write_zip(path: &Path, names: &[String], payload: &[u8]) {
    fn n16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn n32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut crc = 0xffff_ffffu32;
    for byte in payload {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc = !crc;
    let mut archive = fs::File::create(path).unwrap();
    let mut central = Vec::new();
    let mut offset = 0u32;
    for name in names {
        let mut local = Vec::new();
        n32(&mut local, 0x0403_4b50);
        n16(&mut local, 20);
        n16(&mut local, 0x800);
        for _ in 0..3 {
            n16(&mut local, 0);
        }
        n32(&mut local, crc);
        n32(&mut local, payload.len() as u32);
        n32(&mut local, payload.len() as u32);
        n16(&mut local, name.len() as u16);
        n16(&mut local, 0);
        local.extend_from_slice(name.as_bytes());
        local.extend_from_slice(payload);
        archive.write_all(&local).unwrap();
        n32(&mut central, 0x0201_4b50);
        n16(&mut central, 20);
        n16(&mut central, 20);
        n16(&mut central, 0x800);
        for _ in 0..3 {
            n16(&mut central, 0);
        }
        n32(&mut central, crc);
        n32(&mut central, payload.len() as u32);
        n32(&mut central, payload.len() as u32);
        n16(&mut central, name.len() as u16);
        for _ in 0..4 {
            n16(&mut central, 0);
        }
        n32(&mut central, 0);
        n32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
        offset += local.len() as u32;
    }
    archive.write_all(&central).unwrap();
    let mut end = Vec::new();
    n32(&mut end, 0x0605_4b50);
    n16(&mut end, 0);
    n16(&mut end, 0);
    n16(&mut end, names.len() as u16);
    n16(&mut end, names.len() as u16);
    n32(&mut end, central.len() as u32);
    n32(&mut end, offset);
    n16(&mut end, 0);
    archive.write_all(&end).unwrap();
}

fn worker() -> (PathBuf, PathBuf) {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let worker = std::env::var_os("OPENUNPACK_TEST_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            workspace
                .join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe")
        });
    let engine = workspace.join("tools/7zip/7z.dll");
    assert!(
        worker.is_file() && engine.is_file(),
        "Build native Worker and prepare official 7-Zip before acceptance"
    );
    (worker, engine)
}

#[tokio::test]
async fn configured_metadata_budgets_and_safe_paths_are_enforced_by_native_worker() {
    let temporary = tempfile::tempdir().unwrap();
    let data = temporary.path().join("data");
    let (worker, engine) = worker();
    let core = Coordinator::open(data.clone(), worker, engine).unwrap();
    let archive = temporary.path().join("metadata.zip");
    write_zip(
        &archive,
        &["folder/a.txt".into(), "folder/b.txt".into()],
        b"abcd",
    );
    let digest = Sha256::digest(fs::read(&archive).unwrap());
    let mut settings = core.snapshot().unwrap().settings;
    settings.max_entries = 1;
    settings.max_expanded_bytes = 8;
    core.save_settings(settings.clone()).unwrap();
    assert_eq!(
        core.inspect_archive(archive.to_str().unwrap(), None)
            .await
            .unwrap_err(),
        "ResourceLimit"
    );
    settings.max_entries = 2;
    settings.max_expanded_bytes = 7;
    core.save_settings(settings.clone()).unwrap();
    assert_eq!(
        core.inspect_archive(archive.to_str().unwrap(), None)
            .await
            .unwrap_err(),
        "ResourceLimit"
    );
    settings.max_expanded_bytes = 8;
    core.save_settings(settings).unwrap();
    let inspected = core
        .inspect_archive(archive.to_str().unwrap(), None)
        .await
        .unwrap();
    assert_eq!(inspected.total_entries, 2);
    assert_eq!(inspected.total_size, 8);
    assert_eq!(inspected.encrypted_entries, 0);
    assert_eq!(digest, Sha256::digest(fs::read(&archive).unwrap()));
    let unsafe_archive = temporary.path().join("unsafe.zip");
    write_zip(&unsafe_archive, &["../outside.txt".into()], b"x");
    assert_eq!(
        core.inspect_archive(unsafe_archive.to_str().unwrap(), None)
            .await
            .unwrap_err(),
        "UnsafePath"
    );
    assert!(!temporary.path().join("outside.txt").exists());
    assert!(fs::read_dir(&data).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("inspect-")
    }));
    assert!(
        core.snapshot().unwrap().jobs.is_empty(),
        "Inspect creates no extraction task"
    );
    println!(
        "Native configured inspect budgets, unsafe-path rejection, no jobs/output, source hash preservation: PASS"
    );
}

#[tokio::test]
async fn metadata_larger_than_ipc_limit_roundtrips_through_core_and_cleans_owned_spill() {
    let temporary = tempfile::tempdir().unwrap();
    let data = temporary.path().join("data");
    let (worker, engine) = worker();
    let diagnostics = openunpack_core::worker::WorkerClient {
        executable: worker.clone(),
        engine: engine.clone(),
    };
    let core = Coordinator::open(data.clone(), worker, engine).unwrap();
    let archive = temporary.path().join("large-metadata.zip");
    let names: Vec<_> = (0..30_000)
        .map(|i| format!("folder-{i:05}/{}.txt", "x".repeat(220)))
        .collect();
    write_zip(&archive, &names, b"x");
    let digest = Sha256::digest(fs::read(&archive).unwrap());
    let inspected = tokio::time::timeout(
        Duration::from_secs(90),
        core.inspect_archive(archive.to_str().unwrap(), None),
    )
    .await
    .expect("large native inspect deadline")
    .unwrap_or_else(|error| {
        panic!(
            "large metadata must survive authenticated IPC: {error}; Worker diagnostics: {:?}",
            diagnostics.recent_diagnostics()
        )
    });
    assert_eq!(inspected.total_entries, 30_000);
    assert_eq!(inspected.total_size, 30_000);
    assert_eq!(
        inspected.entries.last().unwrap().path.replace('\\', "/"),
        *names.last().unwrap()
    );
    assert!(serde_json::to_vec(&inspected.entries).unwrap().len() > 8 * 1024 * 1024);
    assert!(fs::read_dir(&data).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("inspect-")
    }));
    assert_eq!(digest, Sha256::digest(fs::read(&archive).unwrap()));
    assert!(core.snapshot().unwrap().jobs.is_empty());
    println!(
        "Native Coordinator -> authenticated IPC -> Worker ->7-Zip -> hashed spill -> Core: 30000 entries, >8MiB metadata, scratch cleaned, original hash preserved: PASS"
    );
}

fn write_zip_raw(path: &Path, names: &[&[u8]], payload: &[u8]) {
    fn n16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn n32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut crc = 0xffff_ffffu32;
    for byte in payload {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc = !crc;
    let mut archive = fs::File::create(path).unwrap();
    let mut central = Vec::new();
    let mut offset = 0u32;
    for raw_name in names {
        let mut local = Vec::new();
        n32(&mut local, 0x0403_4b50);
        n16(&mut local, 20);
        n16(&mut local, 0); // bit 11 = 0
        for _ in 0..3 {
            n16(&mut local, 0);
        }
        n32(&mut local, crc);
        n32(&mut local, payload.len() as u32);
        n32(&mut local, payload.len() as u32);
        n16(&mut local, raw_name.len() as u16);
        n16(&mut local, 0);
        local.extend_from_slice(raw_name);
        local.extend_from_slice(payload);
        archive.write_all(&local).unwrap();
        n32(&mut central, 0x0201_4b50);
        n16(&mut central, 20);
        n16(&mut central, 20);
        n16(&mut central, 0); // bit 11 = 0
        for _ in 0..3 {
            n16(&mut central, 0);
        }
        n32(&mut central, crc);
        n32(&mut central, payload.len() as u32);
        n32(&mut central, payload.len() as u32);
        n16(&mut central, raw_name.len() as u16);
        for _ in 0..4 {
            n16(&mut central, 0);
        }
        n32(&mut central, 0);
        n32(&mut central, offset);
        central.extend_from_slice(raw_name);
        offset += local.len() as u32;
    }
    archive.write_all(&central).unwrap();
    let mut end = Vec::new();
    n32(&mut end, 0x0605_4b50);
    n16(&mut end, 0);
    n16(&mut end, 0);
    n16(&mut end, names.len() as u16);
    n16(&mut end, names.len() as u16);
    n32(&mut end, central.len() as u32);
    n32(&mut end, offset);
    n16(&mut end, 0);
    archive.write_all(&end).unwrap();
}

#[tokio::test]
async fn gbk_zip_metadata_inspect_detects_encoding_and_provides_fixed_paths() {
    let temporary = tempfile::tempdir().unwrap();
    let data = temporary.path().join("data");
    let (worker, engine) = worker();
    let core = Coordinator::open(data.clone(), worker, engine).unwrap();
    let archive = temporary.path().join("gbk_test.zip");
    // "测试目录/说明文档.txt" in GBK:
    let gbk_name = [
        0xb2, 0xe2, 0xca, 0xd4, 0xc4, 0xbf, 0xc2, 0xbc, b'/', 0xcb, 0xb5, 0xc3, 0xf7, 0xce, 0xc4,
        0xb5, 0xb5, b'.', b't', b'x', b't',
    ];
    write_zip_raw(&archive, &[&gbk_name], b"hello gbk");
    let inspected = core
        .inspect_archive(archive.to_str().unwrap(), None)
        .await
        .unwrap();
    assert_eq!(inspected.total_entries, 1);
    assert_eq!(inspected.detected_encoding, "GBK");
    assert_eq!(
        inspected.entries[0].path.replace('\\', "/"),
        "测试目录/说明文档.txt"
    );
    println!("GBK ZIP inspect detected encoding and fixed entry path: PASS");
}
