#![no_std]
#![warn(missing_docs)]

//! This crate allows you to wrap a Qei counter in a larger type. This is usefull when your Timer
//! counter is on 16 bit and spend a lot of time overflowing/underflowing.
//! To use this wrapper you have to take samples regularly, but be carefull because the counter
//! **should not** change for more than (2^16 - 1)/2 between two samples otherwise we can not
//! detect overflows/underflows.
//!
//! The internal counter is an i64 which should be enough for most use cases.
//!
//! An example is provided for the stm32f103 µcontroller in this repository.

extern crate embedded_hal;

const THRESHOLD: u16 = 32768;

/// The error returned when we update the internal counter
// TODO : Implement all error traits
#[derive(Debug)]
pub enum SamplingError {
    /// The sample were taken too far apart : you have to make sure that the samples were at a
    /// distance of (2^16-1)/2 maximum.
    SampleTooFar,
}

/// Extend a Qei peripherals by tracking overflows and underflows.
#[derive(Debug)]
pub struct QeiManager {
    counter: i64,
    previous_count: u16,
}

impl QeiManager {
    /// Create a new Qei from an existing one.
    /// The implemntation assume that the counter can't change for more than (2^16-1)/2, because
    /// otherwise we can't detect overflows/underflows
    pub fn new() -> QeiManager {
        QeiManager {
            counter: 0,
            previous_count: 0,
        }
    }

    /// Take a new sample from the Qei and update the internal counter.
    pub fn sample(&mut self, count: u16) -> Result<(), SamplingError> {
        // let count = self.qei.count().into();
        self.update(count)
    }

    /// Take a new sample from the Qei and update the internal counter, unwrapping all errors.
    pub fn sample_unwrap(&mut self, count: u16) {
        // let count = self.qei.count().into();
        self.update(count).unwrap();
    }

    #[allow(dead_code)]
    pub(crate) fn update_unwrap(&mut self, current_count: u16) {
        self.update(current_count).unwrap();
    }

    pub(crate) fn update(&mut self, current_count: u16) -> Result<(), SamplingError> {
        if current_count == self.previous_count {
            return Ok(());
        } else if self.previous_count < current_count {
            if current_count - self.previous_count < THRESHOLD {
                // Counterclockwise rotation no overflow
                self.counter += (current_count - self.previous_count) as i64;
            } else if current_count - self.previous_count > THRESHOLD {
                // Clockwise rotation underflow
                self.counter -= (u16::max_value() - current_count + self.previous_count + 1) as i64;
            } else {
                // The constraint was not resepected
                return Err(SamplingError::SampleTooFar);
            }
        } else {
            if self.previous_count - current_count < THRESHOLD {
                // Clockwise rotation, no overflow
                self.counter -= (self.previous_count - current_count) as i64;
            } else if self.previous_count - current_count > THRESHOLD {
                // Counterclockwise rotation with overflow
                self.counter += (u16::max_value() - self.previous_count + current_count + 1) as i64;
            } else {
                // The constraint was not respeccted
                return Err(SamplingError::SampleTooFar);
            }
        }
        self.previous_count = current_count;
        Ok(())
    }

    /// Returns the internal counter value
    pub fn count(&self) -> i64 {
        self.counter
    }

    /// Resets the internal counter
    pub fn reset(&mut self) {
        self.counter = 0;
    }
}

#[cfg(test)]
mod test {
    use embedded_hal::{Direction, Qei};
    use QeiManager;

    struct DummyQei {}

    impl Qei for DummyQei {
        type Count = u16;
        fn count(&self) -> u16 {
            0
        }
        fn direction(&self) -> Direction {
            Direction::Downcounting
        }
    }

    #[test]
    fn no_trap() {
        let mut qei = QeiManager::new();
        qei.update_unwrap(55);
        assert_eq!(qei.count(), 55)
    }

    #[test]
    fn underflow() {
        let mut qei = QeiManager::new();
        qei.update_unwrap(5);
        qei.update_unwrap(65532);
        assert_eq!(qei.count(), -4); // -4 et pas -3
        let mut qei = QeiManager::new();
        qei.update_unwrap(5);
        qei.update_unwrap(65535);
        assert_eq!(qei.count(), -1);
    }

    #[test]
    fn overflow() {
        let mut qei = QeiManager::new();
        qei.update_unwrap(65522);
        qei.update_unwrap(55);
        assert_eq!(qei.count(), 55_i64);
        let mut qei = QeiManager::new();
        qei.update_unwrap(65535);
        qei.update_unwrap(0);
        assert_eq!(qei.count(), 0);
        qei.update_unwrap(65535);
        qei.update_unwrap(1);
        assert_eq!(qei.count(), 1);
    }

    #[test]
    fn middle_values() {
        let mut qei = QeiManager::new();
        qei.update_unwrap(13546);
        qei.update_unwrap(13500);
        qei.update_unwrap(15678);
        assert_eq!(qei.count(), 15678);
        let mut qei = QeiManager::new();
        qei.update_unwrap(16000);
        qei.update_unwrap(15000);
        assert_eq!(qei.count(), 15000);
    }

    #[test]
    fn going_back() {
        let mut qei = QeiManager::new();
        qei.update_unwrap(65489);
        qei.update_unwrap(65000);
        assert_eq!(qei.count(), -536); // -536 et pas 535 : 65000 - (-536) doit faire 0
        qei.update_unwrap(63000);
        assert_eq!(qei.count(), -2536); // idem
        qei.update_unwrap(62999);
        assert_eq!(qei.count(), -2537); // idem
    }

    #[test]
    fn no_changes() {
        let mut qei = QeiManager::new();
        qei.update_unwrap(0);
        qei.update_unwrap(0);
        assert_eq!(qei.count(), 0);
    }

    #[test]
    fn small_changes() {
        let mut qei = QeiManager::new();
        qei.update_unwrap(0);
        qei.update_unwrap(u16::max_value());
        assert_eq!(qei.count(), -1);
        let mut qei = QeiManager::new();
        qei.update_unwrap(u16::max_value());
        qei.update_unwrap(0);
        assert_eq!(qei.count(), 0);
        qei.update_unwrap(1);
        assert_eq!(qei.count(), 1);
        qei.update_unwrap(65535);
        assert_eq!(qei.count(), -1);
        qei.update_unwrap(65534);
        assert_eq!(qei.count(), -2);
    }
}
