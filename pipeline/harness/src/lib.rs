use once_cell::sync::OnceCell;
use std::{fmt::Debug, sync::Mutex};

pub trait DynAssertion: Send + Sync {
    fn display(&self);
    fn failed(&self) -> bool;
}
#[derive(Debug)]
pub struct AssertEQ<T> {
    pub left: T,
    pub right: T,
    pub result: bool,
}
#[derive(Debug)]
pub struct Assert {
    pub result: bool,
}

#[derive(Debug)]
pub enum Assertion<T> {
    Eq(AssertEQ<T>),
    Cond(Assert),
}

impl<T: Debug + Clone + Send + Sync> DynAssertion for Assertion<T> {
    fn display(&self) {
        println!("{:?}", self);
    }
    fn failed(&self) -> bool {
        match self {
            Assertion::Eq(a) => !a.result,
            Assertion::Cond(a) => !a.result,
        }
    }
}
pub struct AssertionLog {
    pub assertions: Vec<Box<dyn DynAssertion>>,
}

impl AssertionLog {
    pub fn new() -> Self {
        AssertionLog {
            assertions: Vec::new(),
        }
    }

    pub fn add(&mut self, assertion: Box<dyn DynAssertion>) {
        self.assertions.push(assertion);
    }

    pub fn len(&self) -> usize {
        self.assertions.len()
    }

    pub fn display_failures(&self, start: usize, end: usize) {
        for i in start..end {
            if self.assertions[i].failed() {
                self.assertions[i].display();
            }
        }
    }

    fn summarize(&self, start: usize, end: usize) -> (usize, usize) {
        let mut passed = 0;
        let mut failed = 0;
        for i in start..end {
            if self.assertions[i].failed() {
                failed += 1;
            } else {
                passed += 1;
            }
        }
        (passed, failed)
    }
}

// Static variable with Mutex for thread safety.
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
            drop(log);
            let result = std::panic::catch_unwind(test.test_fn);
            let log = ASSERTION_LOG.get().unwrap().lock().unwrap();
            let end = log.len();
            match result {
                Ok(_) => {
                    let (pass, fail) = log.summarize(start, end);
                    println!(
                        "==> {} ASSERTIONS {} passed {} failed",
                        test.name, pass, fail
                    );
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
            panic!("        {} tests failed", fails);
        }
    }
}

#[macro_export]
macro_rules! zk_test {
    ($suite:expr, $name:ident) => {
        $suite.add_test(stringify!($name), $name);
    };
}

#[macro_export]
macro_rules! zk_suits {
    ($($test:ident),*) => {
        let mut test_suite = TestSuite::new();
        $(
            zk_test!(test_suite, $test);
        )*
        test_suite.run();
    };
}

pub fn inner_assert(cond: bool, file: &str, line: u32) -> bool {
    if !cond {
        println!("Assertion failed at {}:{}", file, line);
    }
    cond
}

pub fn inner_assert_eq<T: PartialEq + std::fmt::Debug>(a: T, b: T, file: &str, line: u32) -> bool {
    if a != b {
        println!("Assertion failed: {:?} != {:?} at {}:{}", a, b, file, line);
        false
    } else {
        true
    }
}
fn tryy() {
    let result = inner_assert(false, file!(), line!());
    let log = ASSERTION_LOG.get().unwrap();
    log.lock()
        .unwrap()
        .add(Box::new(Assertion::<bool>::Cond(Assert { result })));

    let result = inner_assert_eq(1, 2, file!(), line!());
    let log = ASSERTION_LOG.get().unwrap();
    log.lock()
        .unwrap()
        .add(Box::new(Assertion::<i32>::Eq(AssertEQ {
            left: 1,
            right: 2,
            result,
        })));
}

#[macro_export]
macro_rules! assert {
    ($cond:expr) => {
        let result = inner_assert(false, file!(), line!());
        let mut log = $crate::ASSERTION_LOG.get().unwrap();
        log.lock()
            .unwrap()
            .add(Box::new(Assertion::<bool>::Cond(Assert { result })));
    };
}
#[macro_export]
macro_rules! assert_eq {
    ($a:expr, $b:expr) => {
        let result = inner_assert_eq($a, $b, file!(), line!());
        let log = $crate::ASSERTION_LOG.get().unwrap();
        log.lock()
            .unwrap()
            .add(Box::new(Assertion::<i32>::Eq(AssertEQ {
                left: 1,
                right: 2,
                result,
            })));
    };
}
