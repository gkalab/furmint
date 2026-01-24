use fm::fs::ops::DecisionState;
use std::time::{Duration, Instant};

#[test]
fn test_initial_state() {
    let before = Instant::now();
    let d = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: Instant::now(),
    };
    assert!(!d.overwrite_all);
    assert!(!d.skip_all);
    assert!(d.last_update >= before);
}

#[test]
fn test_overwrite_and_skip_flags() {
    let t = Instant::now();
    let mut d = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: t,
    };
    d.overwrite_all = true;
    assert!(d.overwrite_all);
    d.skip_all = true;
    assert!(d.skip_all);
    d.overwrite_all = false;
    assert!(!d.overwrite_all);
}

#[test]
fn test_last_update_mutability() {
    let mut d = DecisionState {
        overwrite_all: false,
        skip_all: false,
        last_update: Instant::now(),
    };
    let old = d.last_update;
    std::thread::sleep(Duration::from_millis(10));
    d.last_update = Instant::now();
    assert!(d.last_update > old);
}
