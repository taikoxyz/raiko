use once_cell::sync::OnceCell;
use std::sync::Mutex;

pub mod assert;
pub use assert::*;

// Static variable with Mutex for thread safety.
pub static TESTS_SUIT: OnceCell<Mutex<TestSuite>> = OnceCell::new();
pub static ASSERTION_LOG: OnceCell<Mutex<AssertionLog>> = OnceCell::new();

struct Test {
    name: &'static str,
    test_fn: fn() -> (),
}

// Struct to manage and run tests
pub struct TestSuite {
    tests: Vec<Test>,
}

impl Default for TestSuite {
    fn default() -> Self {
        Self::new()
    }
}

impl TestSuite {
    // Create a new instance of a test suite
    pub fn new() -> Self {
        Self { tests: Vec::new() }
    }

    // Add a test to the suite
    pub fn add_test(&mut self, name: &'static str, test_fn: fn() -> ()) {
        self.tests.push(Test { name, test_fn });
    }

    // Run all tests in the suite
    pub fn run(&self) {
        let mut fails = 0;
        for test in &self.tests {
            println!("💗 Running test: {}", test.name);
            let log = ASSERTION_LOG.get_or_init(|| Mutex::new(AssertionLog::new()));
            let log = log.lock().unwrap();
            let start = log.len();

            let result = std::panic::catch_unwind(test.test_fn);
            let log = ASSERTION_LOG.get().unwrap().lock().unwrap();
            let end = log.len();

            match result {
                Ok(_) => {
                    let (pass, fail) = log.summarize(start, end);
                    println!("==> {} ASSERTIONS {pass} passed {fail} failed", test.name);
                    if fail > 0 {
                        log.display_failures(start, end);
                        fails += fail;
                    }
                }
                Err(_) => {
                    // TODO zkvm cant catch_unwind
                    // if the tread itself panic! the rest will not be executed
                }
            }
        }
        println!("--———————— 🎉Custom Test Harness🎉——————————");
        if fails > 0 {
            panic!("        {fails} tests failed");
        }
    }
}
