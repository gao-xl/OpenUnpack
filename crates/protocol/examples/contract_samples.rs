use openunpack_protocol::worker::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    name: String,
    dto: String,
    input: Value,
    rust_accepts: bool,
    csharp_accepts: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResultItem {
    name: String,
    accepted: bool,
    output: Option<Value>,
}
fn decode(dto: &str, input: Value) -> Result<Value, serde_json::Error> {
    macro_rules! typed {
        ($t:ty) => {
            serde_json::from_value::<$t>(input).and_then(serde_json::to_value)
        };
    }
    match dto {
        "WorkerRequest" => typed!(WorkerRequest),
        "WorkerEvent" => typed!(WorkerEvent),
        "WorkerHello" => typed!(WorkerHello),
        "WorkerCapabilities" => typed!(WorkerCapabilities),
        "CandidateItem" => typed!(CandidateItem),
        "FileRecord" => typed!(FileRecord),
        "CopyFile" => typed!(CopyFile),
        _ => panic!("unknown DTO"),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[2]);
    if args[1] == "export" {
        fs::create_dir_all(root).unwrap();
        let mut samples = vec![];
        let mut push = |name: &str, dto: &str, input: Value, rust_accepts, csharp_accepts| {
            assert_eq!(
                decode(dto, input.clone()).is_ok(),
                rust_accepts,
                "Rust expectation: {name}"
            );
            samples.push(Sample {
                name: name.into(),
                dto: dto.into(),
                input,
                rust_accepts,
                csharp_accepts,
            });
        };
        let v1 = decode(
            "WorkerRequest",
            json!({"protocolVersion":1,"requestId":"v1","operation":"probe",
            "candidatePasswords":["synthetic-only"],"password":null}),
        )
        .unwrap();
        push("v1-full", "WorkerRequest", v1, true, true);
        push(
            "v1-default-fields",
            "WorkerRequest",
            json!({"protocolVersion":1,"requestId":"defaults","operation":"copy"}),
            true,
            true,
        );
        let v2 = decode("WorkerRequest", json!({"protocolVersion":2,"requestId":"v2","operation":"probe",
            "operationId":"op","generation":u64::MAX,"candidates":[{"candidateId":"saved","source":"vault",
            "revision":u64::MAX,"label":"public-fixture","secret":" synthetic-only "}]})).unwrap();
        push("v2-candidates-u64", "WorkerRequest", v2.clone(), true, true);
        let mut ready = WorkerEvent::default();
        ready.kind = "ready".into();
        ready.hello = Some(WorkerHello::default());
        push(
            "hello-ready",
            "WorkerEvent",
            serde_json::to_value(ready).unwrap(),
            true,
            true,
        );
        for kind in ["progress", "error", "cancelled", "completed"] {
            let mut event = WorkerEvent::default();
            event.protocol_version = 2;
            event.request_id = "v2".into();
            event.operation_id = Some("op".into());
            event.generation = Some(u64::MAX);
            event.kind = kind.into();
            event.error_code = if kind == "error" {
                "Corrupt".into()
            } else {
                "".into()
            };
            event.matched_candidate_id = if kind == "completed" {
                Some("saved".into())
            } else {
                None
            };
            push(
                kind,
                "WorkerEvent",
                serde_json::to_value(event).unwrap(),
                true,
                true,
            );
        }
        let mut manifest = WorkerEvent::default();
        manifest.kind = "completed".into();
        manifest.manifest_path = Some("C:\\fixture\\manifest.json".into());
        manifest.manifest_id = Some("a".repeat(32));
        manifest.manifest_sha256 = Some("b".repeat(64));
        manifest.manifest_byte_length = Some(128 * 1024 * 1024);
        push(
            "manifest-metadata",
            "WorkerEvent",
            serde_json::to_value(manifest).unwrap(),
            true,
            true,
        );
        push(
            "capability-defaults",
            "WorkerCapabilities",
            json!({}),
            true,
            true,
        );
        push(
            "file-u64-max",
            "FileRecord",
            serde_json::to_value(FileRecord {
                relative_path: "unicode/文件.txt".into(),
                size: u64::MAX,
                size_known: true,
                sha256: "b".repeat(64),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            })
            .unwrap(),
            true,
            true,
        );
        push(
            "copy-directory",
            "CopyFile",
            serde_json::to_value(CopyFile {
                source_path: "C:\\fixture".into(),
                relative_path: "empty".into(),
                size: 0,
                size_known: true,
                sha256: "".into(),
                is_directory: true,
                is_corrupted: false,
            })
            .unwrap(),
            true,
            true,
        );
        for (name, value) in [
            ("negative-generation", json!(-1)),
            (
                "generation-overflow",
                serde_json::from_str("18446744073709551616").unwrap(),
            ),
            ("numeric-string-generation", json!("7")),
        ] {
            let mut input = v2.clone();
            input["generation"] = value;
            push(name, "WorkerRequest", input, false, false);
        }
        let mut nullable = v2.clone();
        nullable["operationId"] = Value::Null;
        nullable["generation"] = Value::Null;
        push("nullable-identity", "WorkerRequest", nullable, true, true);
        let mut invalid = v2.clone();
        invalid["files"] = Value::Null;
        push(
            "null-required-list-dto-difference",
            "WorkerRequest",
            invalid,
            false,
            true,
        );
        push(
            "missing-required-request-dto-difference",
            "WorkerRequest",
            json!({}),
            false,
            true,
        );
        push(
            "capacity-int-range-difference",
            "WorkerCapabilities",
            json!({"maxBatchCandidates":2147483648_u64}),
            true,
            false,
        );
        fs::write(
            root.join("samples.json"),
            serde_json::to_vec_pretty(&samples).unwrap(),
        )
        .unwrap();
        println!("Exported {} Rust contract samples", samples.len());
    } else if args[1] == "verify" {
        let samples: Vec<Sample> =
            serde_json::from_slice(&fs::read(root.join("samples.json")).unwrap()).unwrap();
        let results: Vec<ResultItem> =
            serde_json::from_slice(&fs::read(root.join("csharp-results.json")).unwrap()).unwrap();
        assert_eq!(samples.len(), results.len());
        for (sample, result) in samples.iter().zip(results) {
            assert_eq!(sample.name, result.name);
            assert_eq!(
                sample.csharp_accepts, result.accepted,
                "C# expectation {}",
                sample.name
            );
            if sample.rust_accepts && sample.csharp_accepts {
                assert_eq!(
                    decode(&sample.dto, sample.input.clone()).unwrap(),
                    decode(&sample.dto, result.output.unwrap()).unwrap(),
                    "Typed roundtrip {}",
                    sample.name
                );
            }
        }
        println!(
            "PASS {} actual Rust -> C# DTO -> Rust cases; documented compatibility differences retained",
            samples.len()
        );
    } else {
        panic!("expected export or verify")
    }
}
