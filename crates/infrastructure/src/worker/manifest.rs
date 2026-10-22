use crate::paths::{self, check_path, path_string, relative};
use openunpack_protocol::{FileRecord, WorkerEvent, WorkerRequest};
use serde::de::{DeserializeSeed, SeqAccess, Visitor};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs;
use std::{
    collections::{HashMap, HashSet},
    io::{BufReader, Read},
    path::Path,
};

const MAX_MANIFEST_BYTES: u64 = 128 * 1024 * 1024;

struct BoundedRecords(usize);
impl<'de> DeserializeSeed<'de> for BoundedRecords {
    type Value = Vec<FileRecord>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        struct RecordsVisitor(usize);
        impl<'de> Visitor<'de> for RecordsVisitor {
            type Value = Vec<FileRecord>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bounded file records")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut records =
                    Vec::with_capacity(seq.size_hint().unwrap_or(0).min(self.0).min(1024));
                while let Some(record) = seq.next_element::<FileRecord>()? {
                    if records.len() == self.0 {
                        return Err(serde::de::Error::custom("manifest entry limit"));
                    }
                    records.push(record);
                }
                Ok(records)
            }
        }
        deserializer.deserialize_seq(RecordsVisitor(self.0))
    }
}

struct DigestReader<R> {
    inner: R,
    digest: Sha256,
    read: u64,
}
impl<R: Read> Read for DigestReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(bytes)?;
        self.digest.update(&bytes[..count]);
        self.read += count as u64;
        Ok(count)
    }
}

fn record_limit(request: &WorkerRequest) -> usize {
    if matches!(request.operation.as_str(), "extract" | "inspect") {
        request.max_entries.min(1_000_000) as usize
    } else {
        request.files.len().min(1_000_000)
    }
}

fn validate_records(records: &[FileRecord], request: &WorkerRequest) -> Result<(), String> {
    let metadata_only = request.operation == "inspect";
    if metadata_only
        && (request.max_entries == 0
            || request.max_entries > 1_000_000
            || request.max_total_bytes > i64::MAX as u64)
    {
        return Err("WorkerManifestLimit".into());
    }
    if records.len() > record_limit(request) {
        return Err("WorkerManifestLimit".into());
    }
    let mut names = HashMap::new();
    let mut parents = HashSet::new();
    let mut total = 0u64;
    for record in records {
        if !record.size_known && (!metadata_only || record.size != 0 || record.is_directory) {
            return Err("WorkerManifestCorrupted".into());
        }
        if record.relative_path.chars().any(|c| c <= '\u{1f}') {
            return Err("WorkerManifestUnsafePath".into());
        }
        let relative = relative(&record.relative_path).map_err(|_| "WorkerManifestUnsafePath")?;
        if (record.is_directory && (record.size != 0 || !record.sha256.is_empty()))
            || (!record.is_directory
                && if metadata_only {
                    !record.sha256.is_empty()
                } else {
                    record.sha256.len() != 64
                        || !record.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                })
        {
            return Err("WorkerManifestCorrupted".into());
        }
        let key = record.relative_path.replace('\\', "/").to_lowercase();
        if names.insert(key.clone(), record.is_directory).is_some() {
            return Err("WorkerManifestUnsafePath".into());
        }
        for parent in relative
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            parents.insert(path_string(parent).replace('\\', "/").to_lowercase());
        }
        if !record.is_directory {
            total = total
                .checked_add(record.size)
                .ok_or("WorkerManifestLimit")?;
        }
    }
    if names
        .iter()
        .any(|(name, directory)| !directory && parents.contains(name))
    {
        return Err("WorkerManifestUnsafePath".into());
    }
    if total > request.max_total_bytes
        || (!metadata_only && request.operation != "readback" && total > request.max_written_bytes)
    {
        return Err("WorkerManifestLimit".into());
    }
    Ok(())
}

fn validate_completion(
    event: &WorkerEvent,
    records: &[FileRecord],
    request: &WorkerRequest,
) -> Result<(), String> {
    validate_records(records, request)?;
    if request.operation == "inspect" {
        let total = records
            .iter()
            .filter(|r| !r.is_directory)
            .try_fold(0u64, |total, record| total.checked_add(record.size))
            .ok_or("WorkerManifestLimit")?;
        if event.verification_scope != "metadataOnly"
            || event.completed_bytes != total
            || event.total_bytes != total
            || event.total_bytes_known != records.iter().all(|record| record.size_known)
        {
            return Err("WorkerManifestCorrupted".into());
        }
    } else if !event.total_bytes_known {
        return Err("WorkerManifestCorrupted".into());
    }
    Ok(())
}

pub fn consume(request: &WorkerRequest, event: &mut WorkerEvent) -> Result<(), String> {
    let Some(manifest_path) = event.manifest_path.as_ref() else {
        if event.manifest_id.is_some()
            || event.manifest_sha256.is_some()
            || event.manifest_byte_length.is_some()
        {
            return Err("WorkerManifestCorrupted".into());
        }
        return if event.kind == "completed" && request.operation != "probe" {
            validate_completion(event, &event.files, request)
        } else {
            Ok(())
        };
    };
    if event.kind != "completed" || !event.files.is_empty() {
        return Err("WorkerManifestCorrupted".into());
    }
    let id = event
        .manifest_id
        .as_deref()
        .ok_or("WorkerManifestCorrupted")?;
    if id.len() != 32
        || !id
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err("WorkerManifestCorrupted".into());
    }
    let expected_hash = event
        .manifest_sha256
        .as_deref()
        .ok_or("WorkerManifestCorrupted")?;
    if expected_hash.len() != 64 || !expected_hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("WorkerManifestCorrupted".into());
    }
    let length = event
        .manifest_byte_length
        .ok_or("WorkerManifestCorrupted")?;
    if length == 0 || length > MAX_MANIFEST_BYTES {
        return Err("WorkerManifestLimit".into());
    }
    let root = Path::new(&request.destination_directory);
    let file_path = Path::new(manifest_path);
    check_path(root)
        .and_then(|_| check_path(file_path))
        .map_err(|_| "WorkerManifestUnsafePath")?;
    let expected_path = root.join(format!(".openunpack-{id}-manifest.json"));
    if !path_string(file_path).eq_ignore_ascii_case(&path_string(&expected_path)) {
        return Err("WorkerManifestUnsafePath".into());
    }
    let entry = paths::open_manifest_file(file_path).map_err(|_| "WorkerManifestReadFailed")?;
    let file = entry.file();
    let metadata = file.metadata().map_err(|_| "WorkerManifestReadFailed")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("WorkerManifestUnsafePath".into());
        }
    }
    if !metadata.is_file() || metadata.len() != length {
        return Err("WorkerManifestCorrupted".into());
    }
    let mut reader = DigestReader {
        inner: BufReader::with_capacity(65536, file.take(length + 1)),
        digest: Sha256::new(),
        read: 0,
    };
    let mut deserializer = serde_json::Deserializer::from_reader(&mut reader);
    let records = BoundedRecords(record_limit(request))
        .deserialize(&mut deserializer)
        .map_err(|_| "WorkerManifestCorrupted")?;
    deserializer.end().map_err(|_| "WorkerManifestCorrupted")?;
    if reader.read != length
        || !hex::encode(reader.digest.clone().finalize()).eq_ignore_ascii_case(expected_hash)
    {
        return Err("WorkerManifestCorrupted".into());
    }
    validate_completion(event, &records, request)?;
    drop(reader);
    entry.delete().map_err(|_| "WorkerManifestCleanupFailed")?;
    event.files = records;
    event.manifest_path = None;
    event.manifest_id = None;
    event.manifest_sha256 = None;
    event.manifest_byte_length = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_sizes_are_metadata_only_and_cannot_claim_a_known_total() {
        let request: WorkerRequest = serde_json::from_value(serde_json::json!({"protocolVersion":2,"requestId":"unknown-metadata","operation":"inspect","maxEntries":2,"maxTotalBytes":1024})).unwrap();
        let record = FileRecord {
            relative_path: "unknown.txt".into(),
            size_known: false,
            ..Default::default()
        };
        let mut event: WorkerEvent = serde_json::from_value(serde_json::json!({"protocolVersion":2,"requestId":"unknown-metadata","kind":"completed","verificationScope":"metadataOnly","totalBytesKnown":false})).unwrap();
        assert!(validate_completion(&event, std::slice::from_ref(&record), &request).is_ok());
        event.total_bytes_known = true;
        assert_eq!(
            validate_completion(&event, std::slice::from_ref(&record), &request).unwrap_err(),
            "WorkerManifestCorrupted"
        );
        event.total_bytes_known = false;
        let extraction: WorkerRequest = serde_json::from_value(serde_json::json!({"protocolVersion":2,"requestId":"unknown-extract","operation":"extract","maxEntries":2,"maxTotalBytes":1024,"maxWrittenBytes":1024})).unwrap();
        assert!(validate_completion(&event, &[record], &extraction).is_err());
    }
    fn request(root: &Path) -> WorkerRequest {
        serde_json::from_value(serde_json::json!({
            "protocolVersion": 2, "requestId": "fixture", "operation": "extract",
            "destinationDirectory": path_string(root), "maxEntries": 2,
            "maxTotalBytes": 100, "maxWrittenBytes": 100
        }))
        .unwrap()
    }
    fn spill(root: &Path) -> WorkerEvent {
        let id = "a".repeat(32);
        let bytes = serde_json::to_vec(&vec![FileRecord {
            relative_path: "file.txt".into(),
            size: 1,
            size_known: true,
            sha256: "b".repeat(64),
            is_directory: false,
            is_encrypted: false,
            is_corrupted: false,
        }])
        .unwrap();
        let path = root.join(format!(".openunpack-{id}-manifest.json"));
        fs::write(&path, &bytes).unwrap();
        let mut event = WorkerEvent::default();
        event.kind = "completed".into();
        event.manifest_path = Some(path_string(&path));
        event.manifest_id = Some(id);
        event.manifest_sha256 = Some(hex::encode(Sha256::digest(&bytes)));
        event.manifest_byte_length = Some(bytes.len() as u64);
        event
    }
    #[test]
    fn validated_spill_is_consumed_and_removed() {
        let root = tempfile::tempdir().unwrap();
        let mut event = spill(root.path());
        let path = event.manifest_path.clone().unwrap();
        consume(&request(root.path()), &mut event).unwrap();
        assert_eq!(event.files.len(), 1);
        assert!(!Path::new(&path).exists());
    }
    #[test]
    fn missing_changed_or_wrong_digest_manifest_is_rejected_and_preserved() {
        for mode in 0..4 {
            let root = tempfile::tempdir().unwrap();
            let mut event = spill(root.path());
            let path = event.manifest_path.clone().unwrap();
            match mode {
                0 => {
                    fs::remove_file(&path).unwrap();
                }
                1 => event.manifest_sha256 = Some("0".repeat(64)),
                2 => event.manifest_byte_length = Some(1),
                _ => event.manifest_byte_length = Some(MAX_MANIFEST_BYTES + 1),
            }
            assert!(consume(&request(root.path()), &mut event).is_err());
            assert!(event.files.is_empty());
            if mode != 0 {
                assert!(Path::new(&path).exists());
            }
        }
    }
    #[test]
    fn outside_spill_and_unsafe_inline_records_cannot_be_accepted() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let mut event = spill(other.path());
        let outside = event.manifest_path.clone().unwrap();
        assert!(consume(&request(root.path()), &mut event).is_err());
        assert!(Path::new(&outside).exists());
        event = WorkerEvent::default();
        event.kind = "completed".into();
        event.files = vec![FileRecord {
            relative_path: "../outside.txt".into(),
            size: 1,
            size_known: true,
            sha256: "c".repeat(64),
            is_directory: false,
            is_encrypted: false,
            is_corrupted: false,
        }];
        assert!(consume(&request(root.path()), &mut event).is_err());
    }

    #[test]
    fn inspect_operation_allows_files_without_sha256() {
        let inspect_req: WorkerRequest = serde_json::from_value(serde_json::json!({
            "protocolVersion": 2, "requestId": "req-inspect", "operation": "inspect",
            "archivePath": "C:\\test\\archive.zip", "maxEntries": 2, "maxTotalBytes": 1024
        }))
        .unwrap();

        let mut event = WorkerEvent::default();
        event.kind = "completed".into();
        event.verification_scope = "metadataOnly".into();
        event.completed_bytes = 1024;
        event.total_bytes = 1024;
        event.files = vec![
            FileRecord {
                relative_path: "folder".into(),
                size: 0,
                size_known: true,
                sha256: "".into(),
                is_directory: true,
                is_encrypted: false,
                is_corrupted: false,
            },
            FileRecord {
                relative_path: "folder/secret.txt".into(),
                size: 1024,
                size_known: true,
                sha256: "".into(),
                is_directory: false,
                is_encrypted: true,
                is_corrupted: false,
            },
        ];

        assert!(consume(&inspect_req, &mut event).is_ok());
        assert_eq!(event.files.len(), 2);
        // The metadata policy must never weaken actual extraction verification.
        let mut extraction = inspect_req.clone();
        extraction.operation = "extract".into();
        extraction.max_written_bytes = 1024;
        assert_eq!(
            consume(&extraction, &mut event).unwrap_err(),
            "WorkerManifestCorrupted"
        );
    }

    fn inspect_fixture(root: &Path) -> (WorkerRequest, WorkerEvent) {
        let mut request = request(root);
        request.operation = "inspect".into();
        request.max_written_bytes = 0;
        let mut event = WorkerEvent::default();
        event.kind = "completed".into();
        event.verification_scope = "metadataOnly".into();
        event.completed_bytes = 1;
        event.total_bytes = 1;
        event.files = vec![FileRecord {
            relative_path: "folder/file.txt".into(),
            size: 1,
            ..Default::default()
        }];
        (request, event)
    }

    fn metadata_spill(root: &Path, event: &mut WorkerEvent) -> std::path::PathBuf {
        let id = "c".repeat(32);
        let bytes = serde_json::to_vec(&event.files).unwrap();
        let path = root.join(format!(".openunpack-{id}-manifest.json"));
        fs::write(&path, &bytes).unwrap();
        event.files.clear();
        event.manifest_path = Some(path_string(&path));
        event.manifest_id = Some(id);
        event.manifest_sha256 = Some(hex::encode(Sha256::digest(&bytes)));
        event.manifest_byte_length = Some(bytes.len() as u64);
        path
    }

    #[test]
    fn inspect_inline_and_spill_enforce_identical_path_scope_and_budget_checks() {
        for spilled in [false, true] {
            for invalid in 0..11 {
                let root = tempfile::tempdir().unwrap();
                let (mut request, mut event) = inspect_fixture(root.path());
                match invalid {
                    0 => event.files[0].relative_path = "../outside.txt".into(),
                    1 => event.files.push(FileRecord {
                        relative_path: "FOLDER/FILE.TXT".into(),
                        ..event.files[0].clone()
                    }),
                    2 => event.files.push(FileRecord {
                        relative_path: "folder".into(),
                        ..Default::default()
                    }),
                    3 => event.files[0].sha256 = "b".repeat(64),
                    4 => event.verification_scope = "targetReadback".into(),
                    5 => request.max_total_bytes = 0,
                    6 => request.max_entries = 0,
                    7 => event.total_bytes = 2,
                    8 => event.files[0].relative_path = "CON.txt".into(),
                    9 => {
                        event.files[0].is_directory = true;
                    }
                    _ => event.files[0].relative_path = "folder/new\nline.txt".into(),
                }
                let path = spilled.then(|| metadata_spill(root.path(), &mut event));
                assert!(
                    consume(&request, &mut event).is_err(),
                    "accepted invalid metadata case {invalid}, spill={spilled}"
                );
                if let Some(path) = path {
                    assert!(path.exists(), "invalid metadata preserved for inspection");
                }
            }
        }
    }

    #[test]
    fn inspect_large_metadata_spill_is_consumed_completely_without_output_hashes() {
        let root = tempfile::tempdir().unwrap();
        let (mut request, mut event) = inspect_fixture(root.path());
        request.max_entries = 30_000;
        request.max_total_bytes = 30_000;
        event.files = (0..30_000)
            .map(|i| FileRecord {
                relative_path: format!("folder-{i:05}/{}.txt", "x".repeat(220)),
                size: 1,
                ..Default::default()
            })
            .collect();
        event.completed_bytes = 30_000;
        event.total_bytes = 30_000;
        let path = metadata_spill(root.path(), &mut event);
        assert!(event.manifest_byte_length.unwrap() > 8 * 1024 * 1024);
        consume(&request, &mut event).unwrap();
        assert_eq!(event.files.len(), 30_000);
        assert!(
            event
                .files
                .last()
                .unwrap()
                .relative_path
                .starts_with("folder-29999/")
        );
        assert!(event.files.iter().all(|file| file.sha256.is_empty()));
        assert_eq!(event.verification_scope, "metadataOnly");
        assert!(!path.exists());
    }
}

#[cfg(test)]
mod path_policy_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn relative_path_policy_accepts_128_components_and_rejects_129() {
        for separator in ["/", "\\"] {
            assert!(relative(&vec!["part"; 128].join(separator)).is_ok());
            assert_eq!(
                relative(&vec!["part"; 129].join(separator)).unwrap_err(),
                "ResourceLimit"
            );
        }
    }

    #[test]
    fn relative_path_policy_rejects_windows_aliases_and_reserved_names() {
        for invalid in [
            "",
            "/file",
            "\\file",
            "C:\\file",
            "folder//file",
            "./file",
            "folder/../file",
            "folder/file:ads",
            "folder/file\0",
            "folder/file.",
            "folder/file ",
            "CON.txt",
            "prn",
            "NUL",
            "CLOCK$",
            "CONIN$",
            "CONOUT$",
            "COM1.bin",
            "lpt9",
            "COM¹.txt",
            "LPT³",
            ".openunpack-owner.json",
            "folder/.OPENUNPACK-OWNER.JSON",
        ] {
            assert!(relative(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn relative_path_policy_preserves_valid_paths_and_length_boundary() {
        assert_eq!(
            relative("folder\\数据/item.txt").unwrap(),
            PathBuf::from("folder").join("数据").join("item.txt")
        );
        assert!(relative(&"a".repeat(30_000)).is_ok());
        assert_eq!(relative(&"a".repeat(30_001)).unwrap_err(), "UnsafePath");
    }

    #[test]
    fn inspect_manifest_rejects_overdeep_records_before_acceptance() {
        let request: WorkerRequest = serde_json::from_value(serde_json::json!({
            "protocolVersion": 2, "requestId": "inspect-depth-boundary", "operation": "inspect",
            "maxEntries": 1, "maxTotalBytes": 1, "maxWrittenBytes": 1
        }))
        .unwrap();
        let mut record = FileRecord {
            relative_path: vec!["part"; 128].join("/"),
            size: 1,
            size_known: true,
            ..Default::default()
        };
        assert!(validate_records(&[record.clone()], &request).is_ok());
        record.relative_path.push_str("/file");
        assert_eq!(
            validate_records(&[record], &request).unwrap_err(),
            "WorkerManifestUnsafePath"
        );
    }
}
