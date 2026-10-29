use super::*;

fn retained_artifacts(directory: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fs::create_dir_all(directory.join("cache")).unwrap();
    [
        "state.json",
        "events.jsonl",
        "receipt.json",
        "reconciliation.json",
        "preflight-only-proof.events.jsonl",
        "cache/retained-index",
    ]
    .into_iter()
    .map(|name| {
        let path = directory.join(name);
        let bytes = format!("retained bytes for {name}\n").into_bytes();
        fs::write(&path, &bytes).unwrap();
        (path, bytes)
    })
    .collect()
}

fn assert_preserved(retained: &[(PathBuf, Vec<u8>)]) {
    for (path, expected) in retained {
        assert_eq!(
            &fs::read(path).unwrap(),
            expected,
            "refused clean mutated {}",
            path.display()
        );
    }
}

#[test]
#[serial_test::serial]
fn clean_refuses_held_ownership_even_with_force_or_missing_metadata() {
    for missing_metadata in [false, true] {
        for force in [false, true] {
            let directory = tempdir().unwrap();
            let state_dir = directory.path().join(".shipper");
            let mut retained = retained_artifacts(&state_dir);
            let owner =
                shipper_core::lock::LockFile::acquire(&state_dir, Some(directory.path())).unwrap();
            let lock_path = shipper_core::lock::lock_path(&state_dir, Some(directory.path()));
            if missing_metadata {
                fs::remove_file(&lock_path).unwrap();
            } else {
                let mut record: serde_json::Value =
                    serde_json::from_slice(&fs::read(&lock_path).unwrap()).unwrap();
                record["acquired_at"] =
                    serde_json::to_value(Utc::now() - chrono::Duration::days(365)).unwrap();
                let bytes = serde_json::to_vec(&record).unwrap();
                fs::write(&lock_path, &bytes).unwrap();
                retained.push((lock_path.clone(), bytes));
            }
            let error = clean_single_dir(&state_dir, directory.path(), false, force).unwrap_err();
            assert!(format!("{error:#}").contains("OS ownership is active"));
            assert_preserved(&retained);
            assert_eq!(lock_path.exists(), !missing_metadata);
            owner.release().unwrap();
        }
    }
}

#[test]
#[serial_test::serial]
fn clean_without_force_preserves_corrupt_metadata_and_all_artifacts() {
    let directory = tempdir().unwrap();
    let state_dir = directory.path().join(".shipper");
    let mut retained = retained_artifacts(&state_dir);
    let lock_path = shipper_core::lock::lock_path(&state_dir, Some(directory.path()));
    let corrupt = vec![0xff, b'{', b' ', b'\n'];
    fs::write(&lock_path, &corrupt).unwrap();
    retained.push((lock_path.clone(), corrupt));
    assert!(clean_single_dir(&state_dir, directory.path(), false, false).is_err());
    assert_preserved(&retained);
    clean_single_dir(&state_dir, directory.path(), false, true).unwrap();
    assert!(!lock_path.exists());
    for (path, _) in retained {
        assert!(!path.exists());
    }
    let mut guard_path = lock_path.as_os_str().to_os_string();
    guard_path.push(".guard");
    assert!(
        Path::new(&guard_path).exists(),
        "clean leaves the guard inode in place"
    );
}

#[test]
#[serial_test::serial]
fn clean_preserves_cleanup_incomplete_hold_even_with_force_or_corrupt_projection() {
    for source in [
        "event_missing_state",
        "event_corrupt_state",
        "state_corrupt_events",
        "event_before_corrupt_line",
        "event_after_corrupt_line",
    ] {
        for force in [false, true] {
            for keep_receipt in [false, true] {
                let directory = tempdir().unwrap();
                let state_dir = directory.path().join(".shipper");
                let paths = retained_artifacts(&state_dir);
                let state_path = state_dir.join("state.json");
                let event_path = state_dir.join("events.jsonl");
                if source == "state_corrupt_events" {
                    let now = Utc::now();
                    let state = ExecutionState {
                        state_version: "shipper.state.v1".into(),
                        plan_id: "retained-cleanup-plan".into(),
                        registry: Registry::crates_io(),
                        created_at: now,
                        updated_at: now,
                        attempt_history: Vec::new(),
                        packages: BTreeMap::from([(
                            "demo@0.1.0".into(),
                            PackageProgress {
                                name: "demo".into(),
                                version: "0.1.0".into(),
                                attempts: 1,
                                state: PackageState::CleanupIncomplete {
                                    message: "cleanup could not be confirmed".into(),
                                },
                                last_updated_at: now,
                            },
                        )]),
                    };
                    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
                } else {
                    if source == "event_missing_state" {
                        fs::remove_file(&state_path).unwrap();
                    }
                    let event = PublishEvent {
                        timestamp: Utc::now(),
                        package: "demo@0.1.0".into(),
                        event_type: EventType::PackageCleanupIncomplete {
                            message: "cleanup could not be confirmed".into(),
                        },
                    };
                    let raw = format!("{}\n", serde_json::to_string(&event).unwrap());
                    let raw = match source {
                        "event_before_corrupt_line" => format!("{raw}malformed-secret-sentinel\n"),
                        "event_after_corrupt_line" => format!("malformed-secret-sentinel\n{raw}"),
                        _ => raw,
                    };
                    fs::write(&event_path, raw).unwrap();
                }
                let retained: Vec<_> = paths
                    .into_iter()
                    .filter(|(path, _)| path.exists())
                    .map(|(path, _)| {
                        let bytes = fs::read(&path).unwrap();
                        (path, bytes)
                    })
                    .collect();
                let error = clean_single_dir(&state_dir, directory.path(), keep_receipt, force)
                    .unwrap_err();
                assert_eq!(
                    engine::classify_publish_stop(&error),
                    Some(engine::PublishStopClassification::CleanupIncomplete)
                );
                assert!(!format!("{error:#}").contains("malformed-secret-sentinel"));
                assert_preserved(&retained);
                assert_eq!(state_path.exists(), source != "event_missing_state");
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
#[serial_test::serial]
fn clean_force_preserves_positively_live_legacy_holder_and_artifacts() {
    let directory = tempdir().unwrap();
    let state_dir = directory.path().join(".shipper");
    let mut retained = retained_artifacts(&state_dir);
    let lock_path = shipper_core::lock::lock_path(&state_dir, Some(directory.path()));
    let owner = shipper_core::lock::LockFile::acquire(&state_dir, Some(directory.path())).unwrap();
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&lock_path).unwrap()).unwrap();
    owner.release().unwrap();
    record.as_object_mut().unwrap().remove("ownership");
    record["acquired_at"] = serde_json::to_value(Utc::now() - chrono::Duration::days(365)).unwrap();
    let bytes = serde_json::to_vec_pretty(&record).unwrap();
    fs::write(&lock_path, &bytes).unwrap();
    retained.push((lock_path, bytes));
    let error = clean_single_dir(&state_dir, directory.path(), false, true).unwrap_err();
    assert!(format!("{error:#}").contains("LiveLegacy"));
    assert_preserved(&retained);
}
