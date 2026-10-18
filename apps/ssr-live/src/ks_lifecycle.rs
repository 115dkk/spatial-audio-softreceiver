//! Endpoint restoration outlives the output sink and the joined engine.

pub struct RestoreOnDrop<F: FnMut() -> Result<(), String>> {
    restore: F,
    armed: bool,
}

impl<F: FnMut() -> Result<(), String>> RestoreOnDrop<F> {
    /// Arm before hiding: a failed hide may have changed Windows state.
    pub fn new(restore: F) -> Self {
        Self {
            restore,
            armed: true,
        }
    }

    pub fn restore(&mut self) -> Result<(), String> {
        if self.armed {
            (self.restore)()?;
            self.armed = false;
        }
        Ok(())
    }
}

impl<F: FnMut() -> Result<(), String>> Drop for RestoreOnDrop<F> {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!(
                "{}",
                ssr_i18n::supervisor::restore_endpoint_failed(ssr_i18n::cli_language(), error)
            );
        }
    }
}

pub fn positive_seconds(value: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| ssr_i18n::live::invalid_seconds(ssr_i18n::cli_language()).to_owned())
}

pub fn probe_report() -> serde_json::Value {
    serde_json::json!({
        "filter": null, "pin": null, "interface": null,
        "rate": 48_000, "channels": 2, "channel_mask": 3,
        "sample_format": null, "opened": false, "error": null,
        "endpoint_restored": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn seconds_must_be_finite_and_positive() {
        for bad in ["0", "-1", "NaN", "inf", "-inf", "1e999", ""] {
            assert!(positive_seconds(bad).is_err(), "{bad}");
        }
        assert_eq!(positive_seconds("0.01"), Ok(0.01));
    }

    #[test]
    fn restoration_follows_sink_drop_on_unwind() {
        struct Sink<'a>(&'a RefCell<Vec<&'static str>>);
        impl Drop for Sink<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().push("closed");
            }
        }
        let events = RefCell::new(Vec::new());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _restore = RestoreOnDrop::new(|| {
                events.borrow_mut().push("restored");
                Ok(())
            });
            let _sink = Sink(&events);
            panic!("probe failure");
        }));
        assert_eq!(*events.borrow(), ["closed", "restored"]);
    }

    #[test]
    fn a_partial_hide_failure_still_restores() {
        let restored = RefCell::new(false);
        let hide = || -> Result<(), String> {
            let _guard = RestoreOnDrop::new(|| {
                *restored.borrow_mut() = true;
                Ok(())
            });
            Err("visibility changed before the operation failed".into())
        };
        assert!(hide().is_err());
        assert!(*restored.borrow());
    }

    #[test]
    fn explicit_success_disarms_but_failure_is_retried_on_drop() {
        let attempts = RefCell::new(0);
        {
            let mut guard = RestoreOnDrop::new(|| {
                *attempts.borrow_mut() += 1;
                if *attempts.borrow() == 1 {
                    Err("pending".into())
                } else {
                    Ok(())
                }
            });
            assert!(guard.restore().is_err());
        }
        assert_eq!(*attempts.borrow(), 2);
        {
            let mut guard = RestoreOnDrop::new(|| {
                *attempts.borrow_mut() += 1;
                Ok(())
            });
            guard.restore().unwrap();
        }
        assert_eq!(*attempts.borrow(), 3);
    }

    #[test]
    fn early_failure_keeps_a_complete_report() {
        let report = probe_report();
        assert_eq!(report["opened"], false);
        assert_eq!(report["endpoint_restored"], false);
        assert_eq!(report["rate"], 48_000);
        assert!(report["filter"].is_null());
        assert!(report["error"].is_null());
    }
}
